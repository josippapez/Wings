/** Azure DevOps: the PR for the branch (`az repos`), its policies, failed builds, the diff from local git, comments from threads and replies. */
import { cleanLog, counts, detail } from "./checks";
import { azureItems } from "./checklist";
import { azWho } from "./who";
import { cli, git, post } from "./cli";
import { azureApiOrg } from "./parse";
import { pushesOf, type AzureIteration, type Push } from "./pushes";
import { problems, status } from "./state";
import { voteArgs, type AzureVote } from "./vote";
import type { AzureLink, AzurePolicy, AzurePr, AzureRemote, Comment, Counts, FailureDetail, Failing, Icon, Link, ListedPr, Result, Reviewer, Status } from "./types";

/** A thread from the `pullRequestThreads` resource. */
interface AzureThread {
  id: number;
  isDeleted?: boolean;
  status?: string;
  threadContext?: { filePath?: string; rightFileStart?: { line?: number }; rightFileEnd?: { line?: number }; leftFileStart?: { line?: number }; leftFileEnd?: { line?: number } } | null;
  pullRequestThreadContext?: { iterationContext?: { secondComparingIteration?: number } } | null;
  comments?: AzureComment[];
}

interface AzureComment {
  id: number;
  commentType?: string;
  isDeleted?: boolean;
  author?: { displayName?: string };
  content?: string;
  publishedDate: string;
}

/** A record of a build's timeline. */
interface TimelineRecord {
  type: string;
  result: string;
  name: string;
  issues?: { type: string; message?: string }[];
  log?: { id: number } | null;
}

// ---------- finding the PR for a pane ----------

export async function findAzure(cwd: string, remote: AzureRemote, branch: string): Promise<Link | null> {
  const out = await cli(remote.org, "az", [
    "repos", "pr", "list",
    "--organization", remote.org, "--project", remote.project, "--repository", remote.repo,
    "--source-branch", branch, "--status", "all", "--top", "5", "-o", "json",
  ]);
  if (out.code !== 0) {
    const login = /login/i.test(out.stderr);
    problems.set(remote.org, { login, message: login ? "Sign in to Azure DevOps to see this branch's pull request." : out.stderr.trim() || "az failed" });
    return null;
  }
  problems.delete(remote.org);
  const prs = JSON.parse(out.stdout) as AzurePr[];
  // The open PR wins; otherwise the newest one for this branch.
  const pr = prs.find((p) => p.status === "active") ?? prs[0];
  if (!pr) return null;
  return azureLink(pr, remote, cwd);
}

export function azureLink(pr: AzurePr, remote: AzureRemote, cwd: string): AzureLink {
  const base = pr.repository?.webUrl ?? `${remote.org}/${encodeURIComponent(remote.project)}/_git/${encodeURIComponent(remote.repo)}`;
  return { provider: "azure", url: `${base}/pullrequest/${pr.pullRequestId}`, number: pr.pullRequestId, repo: `${remote.project}/${remote.repo}`, remote, cwd, raw: pr };
}

/** A PR by number, for a page opened with only its identifiers. */
export async function azureShow(remote: AzureRemote, number: number): Promise<AzurePr> {
  const out = await cli(remote.org, "az", ["repos", "pr", "show", "--id", String(number), "--organization", remote.org, "-o", "json"]);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
  return JSON.parse(out.stdout) as AzurePr;
}

// ---------- the repo's active PRs ----------

/** Every active PR of the repo (up to 100) in one `az repos pr list`, each marked yours or waiting for you; `az account show` says who you are. */
export async function azureList(cwd: string, remote: AzureRemote): Promise<ListedPr[]> {
  const account = await cli(remote.org, "az", ["account", "show", "--query", "user.name", "-o", "tsv"]);
  const me = account.stdout.trim();
  if (account.code !== 0 || !me) throw new Error(account.stderr.trim() || "az account show failed");
  const out = await cli(remote.org, "az", [
    "repos", "pr", "list", "--organization", remote.org, "--project", remote.project, "--repository", remote.repo,
    "--status", "active", "--top", "100", "-o", "json",
  ]);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az repos pr list failed");
  return (JSON.parse(out.stdout) as AzurePr[]).map((pr): ListedPr => {
    const status = { ...azureStatus(pr, { passed: 0, failed: 0, running: 0 }), failing: [] };
    return { link: azureLink(pr, remote, cwd), title: pr.title, head: status.head, ...azWho(pr, me), author: pr.createdBy?.displayName ?? null, status, activity: null };
  });
}

// ---------- status ----------

const short = (ref: string | undefined) => (ref ?? "").replace(/^refs\/heads\//, "");

/** Azure status from the PR plus its policy checks. Reviewer votes: 10 approved, 5 approved with suggestions, -5 waiting for author, -10 rejected. */
function azureStatus(pr: AzurePr, checks: Counts) {
  const votes = (pr.reviewers ?? []).map((r) => r.vote);
  let [label, tone, icon]: [string, Tone, Icon] = ["In review", "info", "pr-open"];
  if (pr.status === "completed") [label, tone, icon] = ["Merged", "merged", "pr-merged"];
  else if (pr.status === "abandoned") [label, tone, icon] = ["Abandoned", "neutral", "pr-closed"];
  else if (pr.isDraft) [label, tone, icon] = ["Draft", "neutral", "pr-draft"];
  else if (pr.mergeStatus === "conflicts") [label, tone] = ["Conflicts", "danger"];
  else if (votes.includes(-10)) [label, tone] = ["Rejected", "danger"];
  else if (checks.failed) [label, tone] = ["Checks failing", "danger"];
  else if (votes.includes(-5)) [label, tone] = ["Waiting for author", "danger"];
  else if (checks.running) [label, tone] = ["Checks running", "warning"];
  else if (votes.some((v) => v > 0)) [label, tone] = ["Approved", "success"];
  const approved = votes.filter((v) => v > 0).length;
  const waiting = votes.filter((v) => v === -5).length;
  const rejected = votes.filter((v) => v === -10).length;
  const review = [approved && `${approved} approved`, waiting && `${waiting} waiting for author`, rejected && `${rejected} rejected`].filter(Boolean).join(", ") || "No votes yet";
  return {
    // A new push changes the source commit, which is what invalidates the cached diff.
    key: pr.lastMergeSourceCommit?.commitId ?? pr.creationDate,
    title: pr.title,
    label, tone, icon, checks, review,
    changes: null,
    branch: `${short(pr.sourceRefName)} → ${short(pr.targetRefName)}`,
    updated: pr.closedDate ?? pr.creationDate,
    base: short(pr.targetRefName),
    head: short(pr.sourceRefName),
    // A completed PR's branch is often deleted, but its commits can still be fetched by id.
    baseCommit: pr.lastMergeTargetCommit?.commitId ?? null,
    headCommit: pr.lastMergeSourceCommit?.commitId ?? null,
  };
}

// Azure shows this policy as the reviewer requirement, not as a check, and it stays queued until someone votes.
const MIN_REVIEWERS_POLICY = "fa4e907d-c16b-4a4c-9dfa-4906e5d171dd";
// Azure reports this one rejected until you pick a merge type when completing, so it gets its own row instead of counting as a failed check.
const MERGE_STRATEGY_POLICY = "fa4e907d-c16b-4a4c-9dfa-4916e5d171ab";
const MERGE_TYPES: Record<string, string> = { allowSquash: "Squash", allowNoFastForward: "Merge", allowRebase: "Rebase", allowRebaseMerge: "Semi-linear merge" };

function mergeStrategy(records: AzurePolicy[]) {
  const r = records.find((p) => p.configuration?.type?.id === MERGE_STRATEGY_POLICY && p.configuration?.isEnabled !== false);
  if (!r) return null;
  const types = Object.keys(MERGE_TYPES).filter((k) => r.configuration!.settings?.[k]).map((k) => MERGE_TYPES[k]).join(" or ");
  return r.status === "approved" ? types : `${types} only, pick it when completing`;
}

/** The status page: the status plus who wrote the PR and its description. */
export async function azureOverview(link: AzureLink) {
  const raw = await azureShow(link.remote, link.number);
  const reviewers = (raw.reviewers ?? []).map((r): Reviewer => ({ name: r.displayName ?? "unknown", vote: r.vote > 0 ? "approved" : r.vote === -10 ? "changes" : r.vote === -5 ? "waiting" : "none" }));
  return { status: await azureChecks({ ...link, raw }), author: raw.createdBy?.displayName ?? "unknown", body: raw.description ?? "", createdAt: raw.creationDate, reviewers };
}

export async function azureChecks(link: AzureLink): Promise<Status> {
  const out = await cli(link.remote.org, "az", ["repos", "pr", "policy", "list", "--id", String(link.number), "--organization", link.remote.org, "-o", "json"]);
  const parsed = (out.code === 0 ? JSON.parse(out.stdout) : []) as AzurePolicy[] | { value?: AzurePolicy[] };
  const records = Array.isArray(parsed) ? parsed : (parsed.value ?? []);
  const policies = records
    .filter((r) => r.status !== "notApplicable" && r.configuration?.isEnabled !== false)
    .filter((r) => ![MIN_REVIEWERS_POLICY, MERGE_STRATEGY_POLICY].includes(r.configuration?.type?.id!));
  const results = policies.map((r): Result => (r.status === "approved" ? "passed" : r.status === "rejected" || r.status === "broken" ? "failed" : "running"));
  // A build policy's context names the build that failed; other policies, like comment requirements, have none.
  const failing = policies
    .filter((_, i) => results[i] === "failed")
    .map((r): Failing => {
      const name = r.configuration?.settings?.displayName ?? r.context?.buildDefinitionName ?? r.configuration?.type?.displayName ?? "Policy";
      const build = r.context?.buildId ?? null;
      return { id: build ?? name, name, build };
    });
  return { ...azureStatus(link.raw, counts(results)), failing, items: azureItems(records, link.raw, link.remote.org, link.remote.project), merge: mergeStrategy(records) };
}

// ---------- failed checks ----------

/** A failed Azure DevOps policy: for a build, its failed tasks from the build timeline and each task's log. */
export async function azureFailure(link: AzureLink, f: Failing): Promise<FailureDetail[]> {
  if (!f.build) return [{ check: f.name, summary: "Not met", log: "", url: link.url }];
  const { org, project: name } = link.remote;
  const invoke = async (resource: string, params: string[]) => {
    const out = await cli(org, "az", [
      "devops", "invoke", "--area", "build", "--resource", resource,
      "--route-parameters", `project=${name}`, `buildId=${f.build}`, ...params,
      "--organization", org, "--api-version", "7.1", "-o", "json",
    ]);
    if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
    return JSON.parse(out.stdout) as unknown;
  };
  const url = `${org}/${encodeURIComponent(name)}/_build/results?buildId=${f.build}&view=logs`;
  const { records = [] } = (await invoke("timeline", [])) as { records?: TimelineRecord[] };
  const tasks = records.filter((r) => r.type === "Task" && r.result === "failed");
  if (!tasks.length) return [{ check: f.name, summary: "Failed", log: "", url }];
  return Promise.all(
    tasks.map(async (r) => {
      const issues = (r.issues ?? []).filter((i) => i.type === "error").map((i) => i.message ?? "");
      let lines = r.log ? cleanLog(((await invoke("logs", [`logId=${r.log.id}`])) as { value?: string[] }).value ?? []) : [];
      // A script task's log starts with the task's description and the script; its output follows this line.
      const start = lines.findIndex((l) => l.includes("Starting Command Output"));
      if (start >= 0) lines = lines.slice(start + 1);
      return detail(f.name, r.name, lines, issues, url);
    }),
  );
}

// ---------- diff and comments ----------

export async function azureDiff(link: AzureLink) {
  const { base, head, baseCommit, headCommit } = status.get(link.url) as Status;
  // Fetch only the PR's two commits, then diff from their merge base, like the PR page does.
  const [from, to] = baseCommit && headCommit ? [baseCommit, headCommit] : [base!, head!];
  await git(link.cwd, ["fetch", "--quiet", "--end-of-options", "origin", from, to]);
  const range = baseCommit && headCommit ? `${from}...${to}` : `origin/${base}...origin/${head}`;
  const patch = await git(link.cwd, ["diff", "--no-color", "--no-ext-diff", range]);
  return { patch: `${patch}\n`, comments: await azureComments(link) };
}

const THREAD_STATES: Record<string, Comment["thread"]> = {
  active: { state: "active", label: "Active" },
  pending: { state: "active", label: "Pending" },
  fixed: { state: "resolved", label: "Resolved" },
  wontFix: { state: "resolved", label: "Won't fix" },
  closed: { state: "resolved", label: "Closed" },
  byDesign: { state: "resolved", label: "By design" },
};

/** The PR's threads, without the diff. `system` adds Azure's own notes, like policy and vote updates, marked as such. */
export async function azureComments(link: AzureLink, system = false): Promise<Comment[]> {
  const threads = await cli(link.remote.org, "az", [
    "devops", "invoke", "--area", "git", "--resource", "pullRequestThreads",
    "--route-parameters", `project=${link.remote.project}`, `repositoryId=${link.remote.repo}`, `pullRequestId=${link.number}`,
    "--organization", link.remote.org, "--api-version", "7.1", "-o", "json",
  ]);
  const value = threads.code === 0 ? ((JSON.parse(threads.stdout) as { value?: AzureThread[] }).value ?? []) : [];
  const comments: Comment[] = [];
  for (const t of value) {
    if (t.isDeleted) continue;
    const ctx = t.threadContext;
    // Threads without a file are comments on the whole PR.
    const path = ctx?.filePath ? ctx.filePath.replace(/^\//, "") : null;
    const right = ctx?.rightFileStart?.line;
    const left = ctx?.leftFileStart?.line;
    const texts = (t.comments ?? []).filter((c) => (c.commentType === "text" || (system && c.commentType === "system")) && !c.isDeleted);
    texts.forEach((c, i) =>
      comments.push({
        id: t.id * 10_000 + c.id,
        // Azure replies can nest; Wings shows them flat under the thread's first comment.
        replyTo: i === 0 ? null : t.id * 10_000 + texts[0].id,
        path,
        line: right ?? left ?? null,
        side: right ? "additions" : "deletions",
        author: c.author?.displayName ?? "unknown",
        body: c.content ?? "",
        createdAt: c.publishedDate,
        url: link.url,
        canReply: i === 0 && c.commentType === "text",
        ...(i === 0 && (right ?? left) ? { range: { start: (right ?? left)!, end: (right ? ctx?.rightFileEnd?.line : ctx?.leftFileEnd?.line) ?? (right ?? left)!, iteration: t.pullRequestThreadContext?.iterationContext?.secondComparingIteration } } : {}),
        ...(i === 0 && t.status && THREAD_STATES[t.status] ? { thread: THREAD_STATES[t.status] } : {}),
        ...(c.commentType === "system" ? { system: true } : {}),
      }),
    );
  }
  return comments;
}

// ---------- pushes and snippets ----------

/** The PR's pushes with their commits: every iteration and its commits in one call, not one call per push. */
export async function azurePushes(link: AzureLink): Promise<Push[]> {
  const out = await cli(link.remote.org, "az", [
    "devops", "invoke", "--area", "git", "--resource", "pullRequestIterations",
    "--route-parameters", `project=${link.remote.project}`, `repositoryId=${link.remote.repo}`, `pullRequestId=${link.number}`,
    "--query-parameters", "includeCommits=true",
    "--organization", link.remote.org, "--api-version", "7.1", "-o", "json",
  ]);
  return out.code === 0 ? pushesOf((JSON.parse(out.stdout) as { value?: AzureIteration[] }).value ?? []) : [];
}

/** A file as it was at a commit, from the project's own clone; a commit it doesn't have is fetched once. Null when neither works. */
export async function azureFileAt(link: AzureLink, sha: string, path: string): Promise<string | null> {
  if (!/^[0-9a-f]{40}$/.test(sha)) return null;
  const show = () => git(link.cwd, ["show", `${sha}:${path}`]);
  try {
    return await show();
  } catch {
    try {
      await git(link.cwd, ["fetch", "--quiet", "--end-of-options", "origin", sha]);
      return await show();
    } catch {
      return null;
    }
  }
}

// ---------- replies ----------

/** Azure comment ids are `thread * 10000 + comment`, as `azureDiff` builds them. */
export async function azureReply(link: AzureLink, root: Comment, body: string): Promise<Comment> {
  const { org, project: name, repo } = link.remote;
  const thread = Math.floor(root.id / 10_000);
  const url = `${azureApiOrg(org)}/${encodeURIComponent(name)}/_apis/git/repositories/${encodeURIComponent(repo)}/pullRequests/${link.number}/threads/${thread}/comments?api-version=7.1`;
  const c = (await post("az", url, { content: body, parentCommentId: root.id % 10_000, commentType: 1 })) as AzureComment;
  return { ...root, id: thread * 10_000 + c.id, replyTo: root.id, author: c.author?.displayName ?? "you", body: c.content ?? body, createdAt: c.publishedDate, canReply: false };
}

// ---------- voting ----------

/** Casts your vote through `az repos pr set-vote`, which works out your reviewer identity itself. */
export async function azureVote(link: AzureLink, vote: AzureVote): Promise<void> {
  const out = await cli(link.remote.org, "az", voteArgs(link.number, link.remote.org, vote));
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
}
