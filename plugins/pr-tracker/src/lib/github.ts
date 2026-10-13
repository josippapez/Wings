/** GitHub: the PR from the transcript or `gh pr list`, its status and failed Actions jobs, the diff, comments and replies. */
import { cleanLog, counts, detail } from "./checks";
import { failingOf, ghItem, type GhCheck } from "./checklist";
import { ghWho } from "./who";
import { cli, post } from "./cli";
import { problems } from "./state";
import type { Comment, FailureDetail, Failing, GithubLink, GithubRemote, Icon, Link, ListedPr, Result, Reviewer, Status } from "./types";

const GH_FIELDS =
  "number,title,state,isDraft,reviewDecision,mergeable,statusCheckRollup,additions,deletions,changedFiles,headRefName,baseRefName,url,updatedAt";
/** What `gh pr list` returns per PR: the status fields, plus the comment and review counts, the author and who is asked to review. One call covers every PR of the repo. */
const GH_LIST_FIELDS = `${GH_FIELDS},comments,reviews,author,reviewRequests`;
// No token prints `gh auth login` or GH_TOKEN; an expired token that gh can renew prints "Try authenticating with:
// gh auth refresh -h <host>" (cli/cli internal/ghcmd/cmd.go). A missing scope also suggests `gh auth refresh`, but
// not after "Try authenticating with:", so it doesn't count.
const GH_LOGGED_OUT = /gh auth login|GH_TOKEN|Try authenticating with:\s+gh auth refresh\b/;

const DOTCOM = "github.com";

/** The host a PR URL is on: github.com or a GitHub Enterprise Server. */
const hostOf = (url: string) => new URL(url).host;
/** The key of a host's login problem and missing-CLI note: "github" for github.com, as it always was. */
export const ghKey = (host: string) => (host === DOTCOM ? "github" : `github:${host}`);
/** `owner/repo` from a PR URL on any host. */
const repoOf = (url: string) => new URL(url).pathname.match(/^\/([^/]+\/[^/]+)\/pull\//)?.[1] ?? "";
/** `gh api` against the PR's host; gh's default is github.com. */
const apiArgs = (host: string, path: string, ...rest: string[]) => ["api", ...(host === DOTCOM ? [] : ["--hostname", host]), path, ...rest];
/** gh finds the repo of a folder by its remotes, but names the repo outright for an Enterprise host, where a remote that gh isn't signed in to is skipped. */
const repoArgs = (remote: GithubRemote) => (remote.host === DOTCOM ? [] : ["--repo", `${remote.host}/${remote.path}`]);

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

// ---------- finding the PR for a pane ----------

export async function fromTranscript(sessionId: string): Promise<Link | null> {
  try {
    const last = (await wings.transcript(sessionId, ["pr-link"], { last: 1 })).at(-1) as { prUrl?: string; prNumber: number; prRepository: string } | undefined;
    if (last?.prUrl) return { provider: "github", url: last.prUrl, number: last.prNumber, repo: last.prRepository };
  } catch {
    // A brand-new session may not have a transcript yet.
  }
  return null;
}

/** Remembers a logged-out `gh`, so the pane offers a sign-in button instead of an error. */
function checkGhLogin(out: { code: number | null; stderr: string }, host: string) {
  if (out.code !== 0 && GH_LOGGED_OUT.test(out.stderr)) {
    problems.set(ghKey(host), { login: true, message: `Sign in to ${host === DOTCOM ? "GitHub" : host} to see this branch's pull request.` });
  } else if (out.code === 0) {
    problems.delete(ghKey(host));
  }
}

export async function findGithub(cwd: string, remote: GithubRemote, branch: string): Promise<Link | null> {
  const out = await cli(ghKey(remote.host), "gh", ["pr", "list", ...repoArgs(remote), "--head", branch, "--state", "all", "--limit", "1", "--json", "url,number"], { cwd });
  checkGhLogin(out, remote.host);
  const pr = out.code === 0 ? (JSON.parse(out.stdout) as { url: string; number: number }[])[0] : null;
  if (!pr) return null;
  return { provider: "github", url: pr.url, number: pr.number, repo: repoOf(pr.url) };
}

// ---------- status ----------

async function ghView(url: string, fields: string) {
  // A Claude pane can know its PR from the transcript without gh, so this can be the first gh call.
  const out = await cli(ghKey(hostOf(url)), "gh", ["pr", "view", url, "--json", fields]);
  checkGhLogin(out, hostOf(url));
  if (out.code !== 0) throw new Error(out.stderr.trim() || "gh failed");
  return JSON.parse(out.stdout) as GhPr;
}

export async function githubStatus(link: Pick<Link, "url">): Promise<Status> {
  return githubStatusOf(await ghView(link.url, GH_FIELDS));
}

/** The status page: the status plus who wrote the PR and its description. */
export async function githubOverview(link: Pick<Link, "url">) {
  const pr = await ghView(link.url, `${GH_FIELDS},body,author,createdAt,latestReviews`);
  const votes: Record<string, Reviewer["vote"] | undefined> = { APPROVED: "approved", CHANGES_REQUESTED: "changes" };
  const reviewers = (pr.latestReviews ?? []).flatMap((r): Reviewer[] => (votes[r.state] ? [{ name: r.author?.login ?? "unknown", vote: votes[r.state]! }] : []));
  return { status: githubStatusOf(pr), author: pr.author?.login ?? "unknown", body: pr.body ?? "", createdAt: pr.createdAt, reviewers };
}

function githubStatusOf(pr: GhPr): Status {
  const items = (pr.statusCheckRollup ?? []).map((c, i) => ghItem(c, hostOf(pr.url), i));
  const results = items.map((c): Result => (c.state === "failed" ? "failed" : c.state === "running" || c.state === "queued" ? "running" : "passed"));
  const checks = counts(results);
  // A GitHub Actions job on the PR's own host has a log; other apps' checks only link to their own page.
  const failing = failingOf(items);
  let [label, tone, icon]: [string, Tone, Icon] = ["Open", "info", "pr-open"];
  if (pr.state === "MERGED") [label, tone, icon] = ["Merged", "merged", "pr-merged"];
  else if (pr.state === "CLOSED") [label, tone, icon] = ["Closed", "neutral", "pr-closed"];
  else if (pr.isDraft) [label, tone, icon] = ["Draft", "neutral", "pr-draft"];
  else if (pr.mergeable === "CONFLICTING") [label, tone] = ["Conflicts", "danger"];
  else if (checks.failed) [label, tone] = ["Checks failing", "danger"];
  else if (pr.reviewDecision === "CHANGES_REQUESTED") [label, tone] = ["Changes requested", "danger"];
  else if (checks.running) [label, tone] = ["Checks running", "warning"];
  else if (pr.reviewDecision === "APPROVED") [label, tone] = ["Approved", "success"];
  else if (pr.reviewDecision === "REVIEW_REQUIRED") [label, tone] = ["In review", "info"];
  const review =
    ({ APPROVED: "Approved", CHANGES_REQUESTED: "Changes requested", REVIEW_REQUIRED: "Waiting for review" } as Record<string, string | undefined>)[pr.reviewDecision] ??
    "No review yet";
  return {
    key: pr.updatedAt,
    title: pr.title,
    label, tone, icon, checks, review, failing, items,
    changes: `+${pr.additions} −${pr.deletions} in ${pr.changedFiles} files`,
    branch: `${pr.headRefName} → ${pr.baseRefName}`,
    updated: pr.updatedAt,
  };
}

// ---------- the repo's open PRs ----------

/** The login `gh` is signed in as, per host, which marks the PRs you wrote and the ones that wait for you. */
const logins = new Map<string, string>();

async function me(host: string): Promise<string> {
  let login = logins.get(host);
  if (!login) {
    const out = await cli(ghKey(host), "gh", apiArgs(host, "user", "--jq", ".login"));
    checkGhLogin(out, host);
    login = out.stdout.trim();
    if (out.code !== 0 || !login) throw new Error(out.stderr.trim() || "gh api user failed");
    logins.set(host, login);
  }
  return login;
}

/** One `gh pr list` for every open PR of the repo (up to 100) and one for the ones asking for your review, each marked yours or waiting for you. */
export async function githubList(cwd: string, remote: GithubRemote): Promise<ListedPr[]> {
  const [user, out, asked] = await Promise.all([
    me(remote.host),
    cli(ghKey(remote.host), "gh", ["pr", "list", ...repoArgs(remote), "--state", "open", "--limit", "100", "--json", GH_LIST_FIELDS], { cwd }),
    // `reviewRequests` has no login for a team, so this search says which PRs ask for you or one of your teams.
    cli(ghKey(remote.host), "gh", ["pr", "list", ...repoArgs(remote), "--state", "open", "--limit", "100", "--search", "review-requested:@me", "--json", "number"], { cwd }),
  ]);
  const requested = new Set(asked.code === 0 ? (JSON.parse(asked.stdout) as { number: number }[]).map((p) => p.number) : []);
  checkGhLogin(out, remote.host);
  if (out.code !== 0) throw new Error(out.stderr.trim() || "gh pr list failed");
  return (JSON.parse(out.stdout) as GhPr[]).map((pr): ListedPr => ({
    link: { provider: "github", url: pr.url, number: pr.number, repo: repoOf(pr.url) },
    title: pr.title,
    head: pr.headRefName,
    ...ghWho(pr, user, requested),
    author: pr.author?.login ?? null,
    status: githubStatusOf(pr),
    activity: (pr.comments?.length ?? 0) + (pr.reviews?.length ?? 0),
  }));
}

// ---------- failed checks ----------

/** A failed GitHub Actions job: its failed step and the end of its log. Unlike `gh run view`, this works while the rest of the run is still going. */
export async function githubFailure(link: GithubLink, f: Failing): Promise<FailureDetail[]> {
  if (!f.job) return [{ check: f.name, summary: f.summary ?? "Failed", log: "", url: f.url }];
  const [key, host] = [ghKey(hostOf(link.url)), hostOf(link.url)];
  const [job, log] = await Promise.all([
    cli(key, "gh", apiArgs(host, `repos/${link.repo}/actions/jobs/${f.job}`)),
    // The log keeps the CI's colours, which gh only prints with this flag.
    cli(key, "gh", apiArgs(host, `repos/${link.repo}/actions/jobs/${f.job}/logs`, "--allow-escape-sequences")),
  ]);
  if (log.code !== 0) throw new Error(log.stderr.trim().split("\n").at(-1) || "gh api failed");
  const step = job.code === 0 ? (JSON.parse(job.stdout) as { steps?: { name: string; conclusion: string }[] }).steps?.find((s) => s.conclusion === "failure")?.name : null;
  let lines = cleanLog(log.stdout.split("\n"));
  // The runner's own cleanup follows the failed step and would push the error out of the tail.
  const cleanup = lines.indexOf("Post job cleanup.");
  if (cleanup >= 0) lines = lines.slice(0, cleanup);
  return [detail(f.name, step ?? null, lines, [], f.url)];
}

// ---------- diff and comments ----------

export async function githubDiff(link: GithubLink) {
  const [diff, comments] = await Promise.all([cli(ghKey(hostOf(link.url)), "gh", ["pr", "diff", link.url]), githubComments(link)]);
  if (diff.code !== 0) throw new Error(diff.stderr.trim() || "gh pr diff failed");
  return { patch: diff.stdout, comments };
}

/** The PR's review threads and its comments on the whole change, without the diff. */
export async function githubComments(link: GithubLink): Promise<Comment[]> {
  const host = hostOf(link.url);
  const [comments, discussion] = await Promise.all([
    cli(ghKey(host), "gh", apiArgs(host, `repos/${link.repo}/pulls/${link.number}/comments`, "--paginate", "--slurp")),
    cli(ghKey(host), "gh", ["pr", "view", link.url, "--json", "comments,reviews"]),
  ]);
  const list = comments.code === 0 ? (JSON.parse(comments.stdout) as GhComment[][]).flat() : [];
  const pr: GhDiscussion = discussion.code === 0 ? (JSON.parse(discussion.stdout) as GhDiscussion) : { comments: [], reviews: [] };
  // Comments on the whole PR: conversation comments plus review summaries. Their ids are GraphQL strings,
  // so they get negative numbers that can't clash with the line comments' ids.
  const general = [
    ...pr.comments.map((c) => ({ author: c.author?.login, body: c.body, createdAt: c.createdAt, url: c.url })),
    ...pr.reviews.filter((r) => r.body).map((r) => ({ author: r.author?.login, body: r.body, createdAt: r.submittedAt, url: link.url })),
  ].map((c, i): Comment => ({ id: -(i + 1), replyTo: null, path: null, line: null, side: "additions", ...c, author: c.author ?? "unknown" }));
  return [...general, ...list.map(githubComment)];
}

/** A line comment from the REST API. GitHub threads only line comments, so only a thread's first one takes replies, on github.com and on an Enterprise host alike. */
function githubComment(c: GhComment): Comment {
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

// ---------- replies and approval ----------

/** `wings.post` names the Enterprise host for `gh`; github.com is gh's default. */
const postOptions = (url: string) => (hostOf(url) === DOTCOM ? undefined : { host: hostOf(url) });

export async function githubReply(link: GithubLink, root: Comment, body: string) {
  return githubComment((await post("gh", `repos/${link.repo}/pulls/${link.number}/comments/${root.id}/replies`, { body }, postOptions(link.url))) as GhComment);
}

/** Approves as the signed-in user. The manifest pins the review's `event` to APPROVE, so this call can't request changes or comment. */
export async function githubApprove(link: GithubLink): Promise<void> {
  await post("gh", `repos/${link.repo}/pulls/${link.number}/reviews`, { event: "APPROVE" }, postOptions(link.url));
}
