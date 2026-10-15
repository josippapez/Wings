/** GitHub: the PR from the transcript or `gh pr list`, its status and failed Actions jobs, the diff, comments and replies. */
import { cached } from "wings-plugin-sdk/async";
import { cli } from "wings-plugin-sdk/cli";
import { between, failingOf, real } from "../checklist";
import { cleanLog, detail } from "../logs";
import { post } from "./post";
import { counts, labelOf, noLog, orElse, same } from "./shared";
import type { Forge } from "./types";
import type { CheckItem, CheckState, Comment, GithubLink, GithubRemote, ListedPr, Failing, Result, Reviewer, Status } from "../types";

const GH_FIELDS =
  "number,title,state,isDraft,reviewDecision,mergeable,statusCheckRollup,additions,deletions,changedFiles,headRefName,baseRefName,url,updatedAt";
/** What `gh pr list` returns per PR: the status fields, plus the comment and review counts, the author and who is asked to review. One call covers every PR of the repo. */
const GH_LIST_FIELDS = `${GH_FIELDS},comments,reviews,author,reviewRequests`;

const DOTCOM = "github.com";

/** The host a PR URL is on: github.com or a GitHub Enterprise Server. */
const hostOf = (url: string) => new URL(url).host;
/** The key of a host's sign-in problem: "github" for github.com, as it always was. */
const ghKey = (host: string) => (host === DOTCOM ? "github" : `github:${host}`);
const hostOfKey = (key: string | undefined) => (key?.startsWith("github:") ? key.slice("github:".length) : DOTCOM);
/** `owner/repo` from a PR URL on any host. */
const repoOf = (url: string) => new URL(url).pathname.match(/^\/([^/]+\/[^/]+)\/pull\//)?.[1] ?? "";
/** `gh api` against the PR's host; gh's default is github.com. */
const apiArgs = (host: string, path: string, ...rest: string[]) => ["api", ...(host === DOTCOM ? [] : ["--hostname", host]), path, ...rest];
/** gh finds the repo of a folder by its remotes, but names the repo outright for an Enterprise host, where a remote that gh isn't signed in to is skipped. */
const repoArgs = (remote: GithubRemote) => (remote.host === DOTCOM ? [] : ["--repo", `${remote.host}/${remote.path}`]);
/** `wings.post` names the Enterprise host for `gh`; github.com is gh's default. */
const postOptions = (url: string) => (hostOf(url) === DOTCOM ? undefined : { host: hostOf(url) });

export const gh = cli("gh", {
  // No token prints `gh auth login` or GH_TOKEN; an expired token that gh can renew prints "Try authenticating with:
  // gh auth refresh -h <host>" (cli/cli internal/ghcmd/cmd.go). A missing scope also suggests `gh auth refresh`, but
  // not after "Try authenticating with:", so it doesn't count.
  signedOut: /gh auth login|GH_TOKEN|Try authenticating with:\s+gh auth refresh\b/,
  signIn: (key) => ["auth", "login", "--web", "--clipboard", "--hostname", hostOfKey(key)],
});

/** `gh` printing JSON, which is never nothing. */
async function ghJson<T>(key: string, args: string[], cwd?: string): Promise<T> {
  const value = await gh.json<T>(args, { key, cwd });
  if (value === null) throw new Error(`gh ${args[0]} printed nothing`);
  return value;
}

// ---------- the CLI's shapes and how they map ----------

/** The fields of `gh pr view --json` the tracker reads. */
interface GhPr {
  number: number;
  url: string;
  title: string;
  body?: string;
  author?: { login?: string };
  comments?: unknown[];
  reviews?: unknown[];
  reviewRequests?: { login?: string }[];
  createdAt?: string;
  latestReviews?: { author?: { login?: string }; state: string }[];
  state: string;
  isDraft: boolean;
  reviewDecision: string;
  mergeable: string;
  statusCheckRollup?: GhCheck[];
  additions: number;
  deletions: number;
  changedFiles: number;
  headRefName: string;
  baseRefName: string;
  updatedAt: string;
}

/** A CheckRun or a StatusContext in `gh pr view --json statusCheckRollup`. */
interface GhCheck {
  __typename?: string;
  state?: string;
  status?: string;
  conclusion?: string;
  detailsUrl?: string;
  targetUrl?: string;
  name?: string;
  context?: string;
  description?: string;
  workflowName?: string;
  startedAt?: string;
  completedAt?: string;
}

/** A line comment from `gh api repos/.../pulls/N/comments`. */
interface GhComment {
  id: number;
  in_reply_to_id?: number;
  path: string;
  line?: number | null;
  side?: string;
  user?: { login?: string };
  body?: string;
  created_at: string;
  html_url: string;
}

interface GhDiscussion {
  comments: { author?: { login?: string }; body: string; createdAt: string; url: string }[];
  reviews: { author?: { login?: string }; body: string; submittedAt: string }[];
}

const GH_FAILED = ["FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"];
const GH_NOTE: Record<string, string> = { TIMED_OUT: "Timed out", CANCELLED: "Cancelled", ACTION_REQUIRED: "Action required", STARTUP_FAILURE: "Failed to start" };

export function ghState(c: GhCheck): CheckState {
  if (c.__typename === "StatusContext") {
    if (c.state === "SUCCESS") return "passed";
    if (c.state === "EXPECTED") return "queued";
    return GH_FAILED.includes(c.state!) ? "failed" : "running";
  }
  if (c.status === "COMPLETED") {
    if (c.conclusion === "SUCCESS" || c.conclusion === "NEUTRAL") return "passed";
    if (c.conclusion === "SKIPPED") return "skipped";
    return GH_FAILED.includes(c.conclusion!) ? "failed" : "running";
  }
  return c.status === "IN_PROGRESS" ? "running" : "queued";
}

/** `host` is the PR's host: only an Actions job there has a log to fetch. `index` is the check's place in the rollup: two contexts can share a name or a link, and each needs its own key. */
export function ghItem(c: GhCheck, host: string, index = 0): CheckItem {
  const url = c.detailsUrl ?? c.targetUrl ?? null;
  const name = c.name ?? c.context ?? "Check";
  const state = ghState(c);
  const job = url?.startsWith(`https://${host}/`) ? url.match(/\/actions\/runs\/\d+\/job\/(\d+)/)?.[1] : undefined;
  const note = c.description || (c.conclusion && GH_NOTE[c.conclusion]) || null;
  const failing: Failing = { id: (url ?? c.name ?? c.context)!, name: c.name ?? c.context, url, job, summary: c.description || null };
  return {
    id: `${name}|${url ?? ""}|${index}`,
    name,
    state,
    // `gh pr view` doesn't say which checks branch protection requires.
    required: null,
    kind: c.workflowName || (c.__typename === "StatusContext" ? "Status check" : "Check run"),
    at: real(c.completedAt) ?? real(c.startedAt),
    seconds: between(c.startedAt, c.completedAt),
    url,
    note,
    failing: state === "failed" ? failing : undefined,
  };
}

const REVIEWS: Record<string, string | undefined> = { APPROVED: "Approved", CHANGES_REQUESTED: "Changes requested", REVIEW_REQUIRED: "Waiting for review" };

export function githubStatusOf(pr: GhPr): Status {
  const items = (pr.statusCheckRollup ?? []).map((c, i) => ghItem(c, hostOf(pr.url), i));
  const results = items.map((c): Result => (c.state === "failed" ? "failed" : c.state === "running" || c.state === "queued" ? "running" : "passed"));
  const checks = counts(results);
  const [label, tone, icon] = labelOf(
    {
      merged: pr.state === "MERGED",
      closed: pr.state === "CLOSED",
      draft: pr.isDraft,
      conflicts: pr.mergeable === "CONFLICTING",
      checks,
      changes: pr.reviewDecision === "CHANGES_REQUESTED",
      approved: pr.reviewDecision === "APPROVED",
      reviewing: pr.reviewDecision === "REVIEW_REQUIRED",
    },
    { rest: "Open" },
  );
  return {
    key: pr.updatedAt,
    title: pr.title,
    label, tone, icon, checks,
    review: REVIEWS[pr.reviewDecision] ?? "No review yet",
    // A GitHub Actions job on the PR's own host has a log; other apps' checks only link to their own page.
    failing: failingOf(items),
    items,
    changes: `+${pr.additions} −${pr.deletions} in ${pr.changedFiles} files`,
    branch: `${pr.headRefName} → ${pr.baseRefName}`,
    updated: pr.updatedAt,
  };
}

const VOTES: Record<string, Reviewer["vote"] | undefined> = { APPROVED: "approved", CHANGES_REQUESTED: "changes" };

/** The reviewers who gave a verdict, from `latestReviews`: a comment-only review isn't one. */
export function githubReviewers(reviews: GhPr["latestReviews"] = []): Reviewer[] {
  return reviews.flatMap((r): Reviewer[] => {
    const vote = VOTES[r.state];
    return vote ? [{ name: r.author?.login ?? "unknown", vote }] : [];
  });
}

/**
 * Whose a listed PR is. `me` is the login `gh api user` gives. `reviewRequests` also lists teams, which have a slug and no
 * login, so `requested` holds the numbers `gh pr list --search review-requested:@me` found: that search includes your teams.
 */
export function ghWho(pr: Pick<GhPr, "number" | "author" | "reviewRequests">, me: string, requested: ReadonlySet<number> = new Set()) {
  return { mine: same(pr.author?.login, me), reviewing: requested.has(pr.number) || (pr.reviewRequests ?? []).some((r) => same(r.login, me)) };
}

/** Every open PR of a repo, each marked yours or waiting for you. `gh pr list` carries what a PR's own status does, so the list's status stands for it. */
export function githubListed(prs: GhPr[], me: string, requested: ReadonlySet<number>): ListedPr[] {
  return prs.map((pr): ListedPr => ({
    link: { provider: "github", url: pr.url, number: pr.number, repo: repoOf(pr.url) },
    title: pr.title,
    head: pr.headRefName,
    ...ghWho(pr, me, requested),
    author: pr.author?.login ?? null,
    status: githubStatusOf(pr),
    loaded: true,
    activity: (pr.comments?.length ?? 0) + (pr.reviews?.length ?? 0),
  }));
}

/** A line comment from the REST API. GitHub threads only line comments, so only a thread's first one takes replies, on github.com and on an Enterprise host alike. */
export function githubComment(c: GhComment): Comment {
  return {
    id: c.id,
    replyTo: c.in_reply_to_id ?? null,
    path: c.path,
    line: c.line ?? null,
    side: c.side === "LEFT" ? "deletions" : "additions",
    author: c.user?.login ?? "unknown",
    body: c.body ?? "",
    createdAt: c.created_at,
    url: c.html_url,
    canReply: !c.in_reply_to_id,
  };
}

/**
 * The PR's conversation: comments on the whole PR, which are the conversation comments plus the review summaries, then
 * the line comments. The whole-PR ones have GraphQL string ids, so they get negative numbers that can't clash with
 * the line comments' ids.
 */
export function githubConversation(discussion: GhDiscussion, lineComments: GhComment[], url: string): Comment[] {
  const general = [
    ...discussion.comments.map((c) => ({ author: c.author?.login, body: c.body, createdAt: c.createdAt, url: c.url })),
    ...discussion.reviews.filter((r) => r.body).map((r) => ({ author: r.author?.login, body: r.body, createdAt: r.submittedAt, url })),
  ].map((c, i): Comment => ({ id: -(i + 1), replyTo: null, path: null, line: null, side: "additions", ...c, author: c.author ?? "unknown" }));
  return [...general, ...lineComments.map(githubComment)];
}

// ---------- calling it ----------

/** The login `gh` is signed in as, per host, which marks the PRs you wrote and the ones that wait for you. */
const logins = cached(async (host: string) => {
  const login = (await gh.run(apiArgs(host, "user", "--jq", ".login"), { key: ghKey(host) })).trim();
  if (!login) throw new Error("gh api user printed no login");
  return login;
});

/** A Claude pane can know its PR from the transcript without gh, so this can be the first gh call. */
const view = (url: string, fields: string) => ghJson<GhPr>(ghKey(hostOf(url)), ["pr", "view", url, "--json", fields]);

async function comments(link: GithubLink): Promise<Comment[]> {
  const host = hostOf(link.url);
  const key = ghKey(host);
  const [lines, discussion] = await Promise.all([
    orElse(gh.json<GhComment[][]>(apiArgs(host, `repos/${link.repo}/pulls/${link.number}/comments`, "--paginate", "--slurp"), { key }), []),
    orElse(gh.json<GhDiscussion>(["pr", "view", link.url, "--json", "comments,reviews"], { key }), { comments: [], reviews: [] }),
  ]);
  return githubConversation(discussion ?? { comments: [], reviews: [] }, (lines ?? []).flat(), link.url);
}

export const github: Forge<GithubLink, GithubRemote> = {
  problemKey: (remote) => ghKey(remote.host),
  repoKey: (remote) => `github:${remote.host}/${remote.path}`,

  login: {
    cli: gh,
    words: (remote) => ({
      signedOut: `Sign in to ${remote.host === DOTCOM ? "GitHub" : remote.host} to see this branch's pull request.`,
      missing: "PR tracker needs the GitHub CLI (gh) to find this branch's pull request.",
    }),
    /**
     * `gh` doesn't open the browser without a terminal: it prints a one-time code and a URL, so the badge shows the code
     * and Wings opens the URL.
     */
    signIn: (remote, onCode) =>
      gh.signIn(ghKey(remote.host), {
        onOutput(line) {
          const code = line.match(/code(?::| \()\s*([A-Z0-9]{4}-[A-Z0-9]{4})/)?.[1];
          if (code) onCode(code);
          const url = line.match(new RegExp(`https://${remote.host.replace(/\./g, "\\.")}/\\S+`))?.[0];
          if (url) void wings.openUrl(url);
        },
      }),
  },

  async fromSession(sessionId) {
    try {
      const last = (await wings.transcript(sessionId, ["pr-link"], { last: 1 })).at(-1);
      if (last?.type === "pr-link" && last.prUrl) return { provider: "github", url: last.prUrl, number: last.prNumber, repo: last.prRepository };
    } catch {
      // A brand-new session may not have a transcript yet.
    }
    return null;
  },

  async find(cwd, remote, branch) {
    // gh ran and failed for a reason that isn't being signed out: nothing to show.
    const prs = await orElse(gh.json<{ url: string; number: number }[]>(["pr", "list", ...repoArgs(remote), "--head", branch, "--state", "all", "--limit", "1", "--json", "url,number"], { key: ghKey(remote.host), cwd }), null);
    const pr = prs?.[0];
    return pr ? { provider: "github", url: pr.url, number: pr.number, repo: repoOf(pr.url) } : null;
  },

  /** One `gh pr list` for every open PR of the repo (up to 100) and one for the ones asking for your review, each marked yours or waiting for you. */
  async list(cwd, remote) {
    const key = ghKey(remote.host);
    const [user, prs, asked] = await Promise.all([
      logins.get(remote.host),
      ghJson<GhPr[]>(key, ["pr", "list", ...repoArgs(remote), "--state", "open", "--limit", "100", "--json", GH_LIST_FIELDS], cwd),
      // `reviewRequests` has no login for a team, so this search says which PRs ask for you or one of your teams.
      orElse(gh.json<{ number: number }[]>(["pr", "list", ...repoArgs(remote), "--state", "open", "--limit", "100", "--search", "review-requested:@me", "--json", "number"], { key, cwd }), null),
    ]);
    return githubListed(prs, user, new Set((asked ?? []).map((p) => p.number)));
  },

  async linkOf({ url, number, repo }) {
    return { provider: "github", url, number, repo };
  },

  async status(link) {
    return githubStatusOf(await view(link.url, GH_FIELDS));
  },

  async overview(link) {
    const pr = await view(link.url, `${GH_FIELDS},body,author,createdAt,latestReviews`);
    return { status: githubStatusOf(pr), author: pr.author?.login ?? "unknown", body: pr.body ?? "", createdAt: pr.createdAt, reviewers: githubReviewers(pr.latestReviews) };
  },

  async diff(link) {
    const [patch, conversation] = await Promise.all([gh.run(["pr", "diff", link.url], { key: ghKey(hostOf(link.url)) }), comments(link)]);
    return { patch, comments: conversation };
  },

  comments,

  /** A failed GitHub Actions job: its failed step and the end of its log. Unlike `gh run view`, this works while the rest of the run is still going. */
  async failure(link, f) {
    if (!f.job) return noLog(f.name, f.summary ?? "Failed", f.url);
    const host = hostOf(link.url);
    const key = ghKey(host);
    const [job, log] = await Promise.all([
      orElse(gh.json<{ steps?: { name: string; conclusion: string }[] }>(apiArgs(host, `repos/${link.repo}/actions/jobs/${f.job}`), { key }), null),
      // The log keeps the CI's colours, which gh only prints with this flag.
      gh.run(apiArgs(host, `repos/${link.repo}/actions/jobs/${f.job}/logs`, "--allow-escape-sequences"), { key }),
    ]);
    const step = job?.steps?.find((s) => s.conclusion === "failure")?.name;
    let lines = cleanLog(log.split("\n"));
    // The runner's own cleanup follows the failed step and would push the error out of the tail.
    const cleanup = lines.indexOf("Post job cleanup.");
    if (cleanup >= 0) lines = lines.slice(0, cleanup);
    return [detail(f.name, step ?? null, lines, [], f.url)];
  },

  async reply(link, root, body) {
    return githubComment((await post("gh", `repos/${link.repo}/pulls/${link.number}/comments/${root.id}/replies`, { body }, postOptions(link.url))) as GhComment);
  },

  /** Approves as the signed-in user. The manifest pins the review's `event` to APPROVE, so this call can't request changes or comment. */
  async approve(link) {
    await post("gh", `repos/${link.repo}/pulls/${link.number}/reviews`, { event: "APPROVE" }, postOptions(link.url));
  },

  forget: () => logins.clear(),
};
