/** GitHub: the PR from the transcript or `gh pr list`, its status and failed Actions jobs, the diff, comments and replies. */
import { cached, type Cached } from "wings-plugin-sdk/async";
import { cli } from "wings-plugin-sdk/cli";
import { githubPulls, type GithubPulls, type GithubRepo } from "wings-plugin-sdk/github";
import { between, failingOf, real } from "../checklist";
import { cleanLog, detail } from "../logs";
import { stepLines } from "../runlog";
import { diffWindow } from "../snippet";
import { post } from "./post";
import { counts, labelOf, noLog, orElse, same } from "./shared";
import type { Forge } from "./types";
import type { CheckItem, CheckState, Comment, GithubLink, GithubRemote, ListedPr, Failing, Result, Reviewer, Run, RunStep, Status } from "../types";

const GH_FIELDS =
  "number,title,state,isDraft,reviewDecision,mergeable,statusCheckRollup,additions,deletions,changedFiles,headRefName,baseRefName,url,updatedAt";

/**
 * What one GraphQL query reads of each open PR: the status fields of `GH_FIELDS`, the author and the comment and review
 * counts. `reviewRequests` isn't read: it costs a point per repository each time, and the review-requested search that
 * rides along says which PRs ask for you or your team. Every nested connection with a `first` counts, so keep to
 * these two (measured: 1 point per repository for the PRs, 1 more per connection under them).
 */
const GH_SELECTION = `number title state isDraft reviewDecision mergeable additions deletions changedFiles headRefName baseRefName url updatedAt
  author { login } comments { totalCount } reviews { totalCount }
  statusCheckRollup { contexts(first: 100) { nodes { __typename
    ... on CheckRun { name status conclusion detailsUrl startedAt completedAt checkSuite { workflowRun { workflow { name } } } }
    ... on StatusContext { context state targetUrl description } } } }`;
/** A little under the 60 s poll, so each poll reads again while the list and the pane statuses of one poll share one read. */
const REPOS_TTL_MS = 55_000;

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
const ghJson = <T>(key: string, args: string[], cwd?: string) => gh.json<T>(args, { key, cwd });

// ---------- the CLI's shapes and how they map ----------

/** The fields of `gh pr view --json` the tracker reads. */
interface GhPr {
  number: number;
  url: string;
  title: string;
  body?: string;
  author?: { login?: string };
  /** Comments and reviews, which the list counts for its unread marks. */
  activity?: number;
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

/** A PR as `GH_SELECTION` reads it. */
interface GqlPr extends Omit<GhPr, "reviewDecision" | "author" | "activity" | "statusCheckRollup"> {
  reviewDecision: string | null;
  author: { login: string } | null;
  comments: { totalCount: number };
  reviews: { totalCount: number };
  statusCheckRollup: { contexts: { nodes: (GhCheck & { checkSuite?: { workflowRun?: { workflow?: { name?: string } | null } | null } | null })[] } } | null;
}

/** The shape `gh pr view --json` gives, so one mapper serves the list and the PR's own read. */
export function fromGraphql({ author, comments, reviews, reviewDecision, statusCheckRollup, ...pr }: GqlPr): GhPr {
  return {
    ...pr,
    author: author ?? undefined,
    reviewDecision: reviewDecision ?? "",
    activity: comments.totalCount + reviews.totalCount,
    statusCheckRollup: (statusCheckRollup?.contexts.nodes ?? []).map(({ checkSuite, ...c }) => ({ ...c, workflowName: checkSuite?.workflowRun?.workflow?.name })),
  };
}

/** A line comment from `gh api repos/.../pulls/N/comments`. */
interface GhComment {
  id: number;
  in_reply_to_id?: number;
  path: string;
  line?: number | null;
  side?: string;
  /** The first line of a multi-line comment, and the side it is on. The `original_*` ones are where they were when the comment was written, which is all an outdated comment has. */
  start_line?: number | null;
  start_side?: string | null;
  original_line?: number | null;
  original_start_line?: number | null;
  /** The diff from the start of its hunk to the commented line, as it was when the comment was written. */
  diff_hunk?: string;
  user?: { login?: string };
  body?: string;
  created_at: string;
  html_url: string;
}

interface GhDiscussion {
  comments: { author?: { login?: string }; body: string; createdAt: string; url: string }[];
  reviews: { author?: { login?: string }; body: string; submittedAt: string }[];
}

/** An Actions job from `actions/jobs/{id}`. */
interface GhJob {
  id: number;
  name: string;
  status: string;
  conclusion: string | null;
  started_at?: string | null;
  completed_at?: string | null;
  html_url?: string | null;
  steps?: { number: number; name: string; status: string; conclusion: string | null; started_at?: string | null; completed_at?: string | null }[];
}

function jobState(status: string, conclusion: string | null): CheckState {
  if (status !== "completed") return status === "in_progress" ? "running" : "queued";
  if (conclusion === "success" || conclusion === "neutral") return "passed";
  return conclusion === "skipped" ? "skipped" : "failed";
}

/** A check run's job and its steps. GitHub keeps a job's log only once the job has finished, which `runLog` cuts into its steps. */
export function ghRun(j: GhJob): Run {
  const done = j.status === "completed";
  const steps = (j.steps ?? []).map((s): RunStep => ({
    id: String(s.number),
    name: s.name,
    state: jobState(s.status, s.conclusion),
    seconds: between(s.started_at, s.completed_at),
    log: done && s.started_at ? String(s.number) : null,
  }));
  return { url: j.html_url ?? null, ...(done ? {} : { note: "GitHub shows the log once the job finishes." }), jobs: [{ id: String(j.id), name: j.name, stage: null, state: jobState(j.status, j.conclusion), seconds: between(j.started_at, j.completed_at), steps }] };
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
    run: job ? { job } : undefined,
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

/** Whose a listed PR is. `me` is the login `gh api user` gives. `requested` holds the numbers the search `review-requested:@me` found, which includes your teams. */
export function ghWho(pr: Pick<GhPr, "number" | "author">, me: string, requested: ReadonlySet<number>) {
  return { mine: same(pr.author?.login, me), reviewing: requested.has(pr.number) };
}

/** Every open PR of a repo, each marked yours or waiting for you. The list carries what a PR's own status does, so the list's status stands for it. */
export function githubListed(prs: GhPr[], me: string, requested: ReadonlySet<number>): ListedPr[] {
  return prs.map((pr): ListedPr => ({
    link: { provider: "github", url: pr.url, number: pr.number, repo: repoOf(pr.url) },
    title: pr.title,
    head: pr.headRefName,
    ...ghWho(pr, me, requested),
    author: pr.author?.login ?? null,
    status: githubStatusOf(pr),
    loaded: true,
    activity: pr.activity ?? 0,
    updatedAt: pr.updatedAt ?? null,
  }));
}

/** A line comment from the REST API. GitHub threads only line comments, so only a thread's first one takes replies, on github.com and on an Enterprise host alike. */
export function githubComment(c: GhComment): Comment {
  const side = c.side === "LEFT" ? "deletions" : "additions";
  // The hunk is the diff as it was when the comment was written and ends at that line, so the thread's code is already in the comment: no call to read the file. `line` is where the line sits now, which the hunk's numbers don't follow once the PR has moved on.
  const end = c.original_line ?? c.line;
  const first = c.original_start_line ?? c.start_line;
  const snippet = !c.in_reply_to_id && c.diff_hunk && end ? diffWindow(c.diff_hunk, side, first && (c.start_side ?? c.side) === c.side ? first : end, end) : null;
  return {
    id: c.id,
    replyTo: c.in_reply_to_id ?? null,
    path: c.path,
    line: c.line ?? null,
    side,
    author: c.user?.login ?? "unknown",
    by: c.user?.login,
    body: c.body ?? "",
    createdAt: c.created_at,
    url: c.html_url,
    canReply: !c.in_reply_to_id,
    ...(snippet ? { snippet } : {}),
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
  ].map((c, i): Comment => ({ id: -(i + 1), replyTo: null, path: null, line: null, side: "additions", ...c, author: c.author ?? "unknown", by: c.author }));
  return [...general, ...lineComments.map(githubComment)];
}

// ---------- calling it ----------

/** The login `gh` is signed in as, per host, which marks the PRs you wrote and the ones that wait for you. */
const logins = cached(async (host: string) => {
  const login = (await gh.run(apiArgs(host, "user", "--jq", ".login"), { key: ghKey(host) })).trim();
  if (!login) throw new Error("gh api user printed no login");
  return login;
});

/** host → the open PRs of each repo, read through one GraphQL call for every repo asked at once, and kept for a poll so the sidebar lists and the pane statuses share it. */
const readers = new Map<string, Cached<GithubRepo, GithubPulls<GqlPr>>>();

function openPulls(host: string) {
  let reader = readers.get(host);
  if (!reader) {
    reader = cached(githubPulls<GqlPr>(gh, { selection: GH_SELECTION, host, key: ghKey(host) }).get, { ttlMs: REPOS_TTL_MS, key: (r) => `${r.owner}/${r.name}`.toLowerCase() });
    readers.set(host, reader);
  }
  return reader;
}

/** `owner/repo` as the parts a GraphQL `repository` takes. */
const partsOf = (repo: string): GithubRepo => ({ owner: repo.slice(0, repo.indexOf("/")), name: repo.slice(repo.indexOf("/") + 1) });

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

/** The PR a `pr-link` transcript entry records, or null for any other entry. */
export function prLinkOf(entry: TranscriptEntry | undefined): GithubLink | null {
  return entry?.type === "pr-link" && entry.prUrl ? { provider: "github", url: entry.prUrl, number: entry.prNumber, repo: entry.prRepository } : null;
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
      return prLinkOf((await wings.transcript(sessionId, ["pr-link"], { last: 1 })).at(-1));
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

  /** The login `gh` is signed in as, which is also what a mention is written with. */
  async me(remote) {
    const login = await logins.get(remote.host);
    return { ids: [login], handles: [login] };
  },

  /** Every open PR of the repo (up to 100), each marked yours or waiting for you, from one GraphQL call that every repo asked in the same turn shares. */
  async list(_cwd, remote) {
    const [user, { pulls, asked }] = await Promise.all([logins.get(remote.host), openPulls(remote.host).get(partsOf(remote.path))]);
    return githubListed(pulls.map(fromGraphql), user, new Set(asked));
  },

  async linkOf({ url, number, repo }) {
    return { provider: "github", url, number, repo };
  },

  /** An open PR's status is in its repo's list, which the sidebar reads anyway: `gh pr view` is for one the list doesn't have, like a merged one. `fresh` skips a list that's still kept. */
  async status(link, { fresh = false } = {}) {
    // A list that can't be read leaves it to `gh pr view`, which fails the same way when it is a sign-in.
    const open = await openPulls(hostOf(link.url)).get(partsOf(link.repo), { fresh }).catch(() => null);
    const pr = open?.pulls.find((p) => p.number === link.number);
    return githubStatusOf(pr ? fromGraphql(pr) : await view(link.url, GH_FIELDS));
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

  async run(link, ref) {
    const host = hostOf(link.url);
    return ghRun(await gh.json<GhJob>(apiArgs(host, `repos/${link.repo}/actions/jobs/${ref.job}`), { key: ghKey(host) }));
  },

  /** Actions keeps one log per job, which is cut into its steps by when each ran. */
  async runLog(link, ref, step) {
    const host = hostOf(link.url);
    const key = ghKey(host);
    const [job, log] = await Promise.all([
      gh.json<GhJob>(apiArgs(host, `repos/${link.repo}/actions/jobs/${ref.job}`), { key }),
      gh.run(apiArgs(host, `repos/${link.repo}/actions/jobs/${ref.job}/logs`, "--allow-escape-sequences"), { key }),
    ]);
    const steps = job.steps ?? [];
    const parts = stepLines(log.split("\n"), steps.map((s) => ({ start: s.started_at ?? null, end: s.completed_at ?? null })));
    return parts[steps.findIndex((s) => String(s.number) === step.id)] ?? [];
  },

  async reply(link, root, body) {
    return githubComment((await post("gh", `repos/${link.repo}/pulls/${link.number}/comments/${root.id}/replies`, { body }, postOptions(link.url))) as GhComment);
  },

  /** Approves as the signed-in user. The manifest pins the review's `event` to APPROVE, so this call can't request changes or comment. */
  async approve(link) {
    await post("gh", `repos/${link.repo}/pulls/${link.number}/reviews`, { event: "APPROVE" }, postOptions(link.url));
  },

  forget() {
    logins.clear();
    readers.clear();
  },
};
