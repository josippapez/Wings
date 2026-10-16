/** Azure DevOps: the PR for the branch (`az repos`), its policies, failed builds, the diff from local git, comments from threads and replies. */
import { cli, CliError, lastLine } from "wings-plugin-sdk/cli";
import { azureApiOrg, git } from "wings-plugin-sdk/git";
import { between, real } from "../checklist";
import { cleanLog, detail } from "../logs";
import { pushesOf, type AzureIteration } from "../pushes";
import { snippetOf } from "../snippet";
import { voteArgs } from "../vote";
import { post } from "./post";
import { counts, labelOf, noLog, openOrNewest, orElse, same } from "./shared";
import { AccountProblem, type Forge } from "./types";
import type { AzureLink, AzurePr, AzureRemote, CheckItem, CheckState, Comment, Counts, Failing, ListedPr, Result, Reviewer, Status } from "../types";

export const az = cli("az", {
  // `az` words a signed-out state in many ways, and every one says `login`.
  signedOut: /login/i,
  signIn: ["login", "--allow-no-subscriptions", "--output", "none"],
});

// ---------- the CLI's shapes and how they map ----------

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

interface AzurePolicy {
  status: string;
  startedDate?: string;
  completedDate?: string;
  configuration?: {
    isEnabled?: boolean;
    isBlocking?: boolean;
    type?: { id?: string; displayName?: string };
    settings?: Record<string, unknown> & { displayName?: string };
  };
  context?: { buildId?: number; buildDefinitionName?: string };
}

export function azureLink(pr: AzurePr, remote: AzureRemote, cwd: string): AzureLink {
  const base = pr.repository?.webUrl ?? `${remote.org}/${encodeURIComponent(remote.project)}/_git/${encodeURIComponent(remote.repo)}`;
  return { provider: "azure", url: `${base}/pullrequest/${pr.pullRequestId}`, number: pr.pullRequestId, repo: `${remote.project}/${remote.repo}`, remote, cwd, raw: pr };
}

const short = (ref: string | undefined) => (ref ?? "").replace(/^refs\/heads\//, "");

/** Azure status from the PR plus its policy checks. Reviewer votes: 10 approved, 5 approved with suggestions, -5 waiting for author, -10 rejected. */
export function azureStatus(pr: AzurePr, checks: Counts) {
  const votes = (pr.reviewers ?? []).map((r) => r.vote);
  const [label, tone, icon] = labelOf(
    { merged: pr.status === "completed", closed: pr.status === "abandoned", draft: pr.isDraft, conflicts: pr.mergeStatus === "conflicts", rejected: votes.includes(-10), checks, changes: votes.includes(-5), approved: votes.some((v) => v > 0) },
    { closed: "Abandoned", changes: "Waiting for author" },
  );
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

/** Azure's policy types, by the name its policy list gives them. Build and the two below are told apart by what the record holds. */
const AZURE_KINDS: [RegExp, string, CheckItem["needs"]?][] = [
  [/comment/i, "Comment requirements", "unresolved"],
  [/work item/i, "Work item linking"],
  [/required reviewers|automatically included/i, "Required reviewers"],
  [/minimum|number of reviewers/i, "Minimum reviewers"],
  [/merge strategy/i, "Merge strategy"],
  [/build/i, "Build"],
  [/status/i, "Status check"],
];

const azureState = (status: string): CheckState =>
  status === "approved" ? "passed" : status === "rejected" || status === "broken" ? "failed" : status === "running" ? "running" : status === "queued" ? "queued" : "skipped";

/** A pull request's policies as checks. `org` is the organization's URL. A disabled policy isn't one. */
export function azureItems(policies: AzurePolicy[], pr: AzurePr, org: string, project: string): CheckItem[] {
  const reviewers = pr.reviewers ?? [];
  const required = reviewers.filter((r) => r.isRequired);
  const waiting = required.filter((r) => !(r.vote > 0)).map((r) => r.displayName ?? "unknown");
  const approvals = reviewers.filter((r) => r.vote > 0).length;
  return policies
    .filter((p) => p.configuration?.isEnabled !== false)
    .map((p, i): CheckItem => {
      const typeName = p.configuration?.type?.displayName ?? "";
      const settings = p.configuration?.settings ?? {};
      const [, kind, needs] = AZURE_KINDS.find(([re]) => re.test(typeName)) ?? [null, p.context?.buildId ? "Build" : typeName || "Policy"];
      const state = azureState(p.status);
      const build = p.context?.buildId ?? null;
      const name = String(settings.displayName ?? p.context?.buildDefinitionName ?? settings.statusName ?? (typeName || "Policy"));
      let note: string | null = null;
      if (state === "skipped") note = "Doesn't apply to this pull request";
      else if (kind === "Required reviewers" && required.length) {
        note = `${required.length - waiting.length} of ${required.length} required reviewers approved${waiting.length ? `. Waiting for ${waiting.join(", ")}` : ""}`;
      } else if (kind === "Minimum reviewers") {
        const need = Number(settings.minimumApproverCount);
        if (need > 0) note = `${Math.min(approvals, need)} of ${need} approvals`;
      } else if (kind === "Work item linking") note = state === "passed" ? "A work item is linked" : "Link a work item to this pull request";
      else if (kind === "Comment requirements" && state !== "passed") note = "Comment threads are unresolved";
      else if (kind === "Merge strategy" && state !== "passed") note = "Pick the merge type when completing";
      else if (state === "failed" && !build) note = p.status === "broken" ? "The policy hit an error" : "Not met";
      else if (state === "failed") note = p.status === "broken" ? "The build policy hit an error" : "The build failed";
      else if (state === "queued" && build) note = "Waiting for the build to start";
      return {
        id: `${p.configuration?.type?.id ?? "policy"}:${build ?? name}:${i}`,
        name,
        state: kind === "Merge strategy" && state === "failed" ? "queued" : state,
        required: typeof p.configuration?.isBlocking === "boolean" ? p.configuration.isBlocking : null,
        kind,
        at: real(p.completedDate) ?? real(p.startedDate),
        seconds: between(p.startedDate, p.completedDate),
        url: build ? `${org}/${encodeURIComponent(project)}/_build/results?buildId=${build}` : null,
        note,
        needs,
        failing: state === "failed" && build ? { id: build, name, build } : undefined,
      };
    });
}

/** The status of a PR from its policy records. */
export function azureStatusOf(pr: AzurePr, records: AzurePolicy[], remote: AzureRemote): Status {
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
  return { ...azureStatus(pr, counts(results)), failing, items: azureItems(records, pr, remote.org, remote.project), merge: mergeStrategy(records) };
}

export function azureReviewers(pr: AzurePr): Reviewer[] {
  return (pr.reviewers ?? []).map((r): Reviewer => ({ name: r.displayName ?? "unknown", vote: r.vote > 0 ? "approved" : r.vote === -10 ? "changes" : r.vote === -5 ? "waiting" : "none" }));
}

/** Whose a listed PR is. `me` is what `az account show` calls the user, which Azure DevOps knows as `uniqueName`. Any reviewer counts, voted or not, as the sidebar's `--reviewer` query did. */
export function azWho(pr: Pick<AzurePr, "createdBy" | "reviewers">, me: string) {
  return { mine: same(pr.createdBy?.uniqueName, me), reviewing: (pr.reviewers ?? []).some((r) => same(r.uniqueName, me)) };
}

/** Every active PR of the repo, each marked yours or waiting for you. */
export function azureListed(prs: AzurePr[], me: string, remote: AzureRemote, cwd: string): ListedPr[] {
  return prs.map((pr): ListedPr => {
    const status = { ...azureStatus(pr, { passed: 0, failed: 0, running: 0 }), failing: [] };
    return { link: azureLink(pr, remote, cwd), title: pr.title, head: status.head, ...azWho(pr, me), author: pr.createdBy?.displayName ?? null, status, loaded: false, activity: null };
  });
}

const THREAD_STATES: Record<string, Comment["thread"]> = {
  active: { state: "active", label: "Active" },
  pending: { state: "active", label: "Pending" },
  fixed: { state: "resolved", label: "Resolved" },
  wontFix: { state: "resolved", label: "Won't fix" },
  closed: { state: "resolved", label: "Closed" },
  byDesign: { state: "resolved", label: "By design" },
};

/** The PR's threads as comments. `system` adds Azure's own notes, like policy and vote updates, marked as such. Comment ids are `thread * 10000 + comment`. */
export function azureComments(threads: AzureThread[], url: string, system = false): Comment[] {
  const comments: Comment[] = [];
  for (const t of threads) {
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
        url,
        canReply: i === 0 && c.commentType === "text",
        ...(i === 0 && (right ?? left) ? { range: { start: (right ?? left)!, end: (right ? ctx?.rightFileEnd?.line : ctx?.leftFileEnd?.line) ?? (right ?? left)!, iteration: t.pullRequestThreadContext?.iterationContext?.secondComparingIteration } } : {}),
        ...(i === 0 && t.status && THREAD_STATES[t.status] ? { thread: THREAD_STATES[t.status] } : {}),
        ...(c.commentType === "system" ? { system: true } : {}),
      }),
    );
  }
  return comments;
}

/** The comment a reply made, in the thread of `root`. */
export function azureReplied(root: Comment, c: AzureComment, body: string): Comment {
  return { ...root, id: Math.floor(root.id / 10_000) * 10_000 + c.id, replyTo: root.id, author: c.author?.displayName ?? "you", body: c.content ?? body, createdAt: c.publishedDate, canReply: false };
}

// ---------- calling it ----------

/** `az` printing JSON, which is never nothing here. */
const azJson = <T>(org: string, args: string[]) => az.json<T>(args, { key: org });

/** A PR by number, for a page opened with only its identifiers. */
const show = (remote: AzureRemote, number: number) => azJson<AzurePr>(remote.org, ["repos", "pr", "show", "--id", String(number), "--organization", remote.org, "-o", "json"]);

/** A resource of the Azure DevOps REST API, through `az devops invoke`. */
const invoke = (org: string, area: string, resource: string, route: string[], extra: string[] = []) =>
  az.json<unknown>(["devops", "invoke", "--area", area, "--resource", resource, "--route-parameters", ...route, ...extra, "--organization", org, "--api-version", "7.1", "-o", "json"], { key: org });

const prRoute = (link: AzureLink) => [`project=${link.remote.project}`, `repositoryId=${link.remote.repo}`, `pullRequestId=${link.number}`];

async function status(link: AzureLink): Promise<Status> {
  const parsed = await orElse(az.json<AzurePolicy[] | { value?: AzurePolicy[] }>(["repos", "pr", "policy", "list", "--id", String(link.number), "--organization", link.remote.org, "-o", "json"], { key: link.remote.org }), []);
  const records = Array.isArray(parsed) ? parsed : (parsed?.value ?? []);
  return azureStatusOf(link.raw, records, link.remote);
}

async function comments(link: AzureLink, system = false): Promise<Comment[]> {
  const threads = (await orElse(invoke(link.remote.org, "git", "pullRequestThreads", prRoute(link)), null)) as { value?: AzureThread[] } | null;
  return azureComments(threads?.value ?? [], link.url, system);
}

/** A file as it was at a commit, from the project's own clone; a commit it doesn't have is fetched once. Null when neither works. */
async function fileAt(link: AzureLink, sha: string, path: string): Promise<string | null> {
  if (!/^[0-9a-f]{40}$/.test(sha)) return null;
  const read = () => git(link.cwd, ["show", `${sha}:${path}`], { raw: true });
  try {
    return await read();
  } catch {
    try {
      await git(link.cwd, ["fetch", "--quiet", "--end-of-options", "origin", sha]);
      return await read();
    } catch {
      return null;
    }
  }
}

export const azure: Forge<AzureLink, AzureRemote> = {
  problemKey: (remote) => remote.org,
  repoKey: (remote) => `azure:${remote.org}/${remote.project}/${remote.repo}`,

  login: {
    cli: az,
    words: () => ({
      signedOut: "Sign in to Azure DevOps to see this branch's pull request.",
      missing: "PR tracker needs the Azure CLI (az) to find this branch's pull request.",
    }),
    signIn: (remote) => az.signIn(remote.org),
  },

  async find(cwd, remote, branch) {
    let prs: AzurePr[];
    try {
      prs = await azJson<AzurePr[]>(remote.org, [
        "repos", "pr", "list",
        "--organization", remote.org, "--project", remote.project, "--repository", remote.repo,
        "--source-branch", branch, "--status", "all", "--top", "5", "-o", "json",
      ]);
    } catch (error) {
      // az ran and failed for a reason that isn't being signed out: the pane says pull requests are unavailable.
      if (error instanceof CliError && error.kind === "failed" && error.stderr) throw new AccountProblem("unavailable", lastLine(error.message));
      throw error;
    }
    const pr = openOrNewest(prs, (p) => p.status === "active");
    return pr ? azureLink(pr, remote, cwd) : null;
  },

  /** Every active PR of the repo (up to 100) in one `az repos pr list`, each marked yours or waiting for you; `az account show` says who you are. */
  async list(cwd, remote) {
    const me = (await az.run(["account", "show", "--query", "user.name", "-o", "tsv"], { key: remote.org })).trim();
    if (!me) throw new Error("az account show printed no user");
    const prs = await azJson<AzurePr[]>(remote.org, [
      "repos", "pr", "list", "--organization", remote.org, "--project", remote.project, "--repository", remote.repo,
      "--status", "active", "--top", "100", "-o", "json",
    ]);
    return azureListed(prs, me, remote, cwd);
  },

  /** Azure DevOps looks the PR up again, because its status needs the PR's own fields. */
  async linkOf({ remote, number, url, cwd }) {
    if (remote?.kind !== "azure") throw new Error("This tab doesn't say which Azure DevOps repository its pull request is in.");
    return { ...azureLink(await show(remote, number), remote, cwd), url };
  },

  status,

  async overview(link) {
    const raw = await show(link.remote, link.number);
    return { status: await status({ ...link, raw }), author: raw.createdBy?.displayName ?? "unknown", body: raw.description ?? "", createdAt: raw.creationDate, reviewers: azureReviewers(raw) };
  },

  async diff(link, { base, head, baseCommit, headCommit }) {
    // Fetch only the PR's two commits, then diff from their merge base, like the PR page does.
    const [from, to] = baseCommit && headCommit ? [baseCommit, headCommit] : [base!, head!];
    await git(link.cwd, ["fetch", "--quiet", "--end-of-options", "origin", from, to]);
    const range = baseCommit && headCommit ? `${from}...${to}` : `origin/${base}...origin/${head}`;
    const patch = await git(link.cwd, ["diff", "--no-color", "--no-ext-diff", range], { raw: true });
    return { patch, comments: await comments(link) };
  },

  /** The threads of the tab. Azure's own notes ("The reference … was updated") are not comments: `pushes` draws them. */
  comments: (link) => comments(link),

  /** A failed Azure DevOps policy: for a build, its failed tasks from the build timeline and each task's log. */
  async failure(link, f) {
    if (!f.build) return noLog(f.name, "Not met", link.url);
    const { org, project: name } = link.remote;
    const build = (resource: string, route: string[] = []) => invoke(org, "build", resource, [`project=${name}`, `buildId=${f.build}`, ...route]);
    const url = `${org}/${encodeURIComponent(name)}/_build/results?buildId=${f.build}&view=logs`;
    const { records = [] } = ((await build("timeline")) ?? {}) as { records?: TimelineRecord[] };
    const tasks = records.filter((r) => r.type === "Task" && r.result === "failed");
    if (!tasks.length) return noLog(f.name, "Failed", url);
    return Promise.all(
      tasks.map(async (r) => {
        const issues = (r.issues ?? []).filter((i) => i.type === "error").map((i) => i.message ?? "");
        let lines = r.log ? cleanLog((((await build("logs", [`logId=${r.log.id}`])) ?? {}) as { value?: string[] }).value ?? []) : [];
        // A script task's log starts with the task's description and the script; its output follows this line.
        const start = lines.findIndex((l) => l.includes("Starting Command Output"));
        if (start >= 0) lines = lines.slice(start + 1);
        return detail(f.name, r.name, lines, issues, url);
      }),
    );
  },

  /** Azure comment ids are `thread * 10000 + comment`, as `azureComments` builds them. */
  async reply(link, root, body) {
    const { org, project: name, repo } = link.remote;
    const thread = Math.floor(root.id / 10_000);
    const url = `${azureApiOrg(org)}/${encodeURIComponent(name)}/_apis/git/repositories/${encodeURIComponent(repo)}/pullRequests/${link.number}/threads/${thread}/comments?api-version=7.1`;
    return azureReplied(root, (await post("az", url, { content: body, parentCommentId: root.id % 10_000, commentType: 1 })) as AzureComment, body);
  },

  /** Casts your vote through `az repos pr set-vote`, which works out your reviewer identity itself. */
  async vote(link, vote) {
    await az.run(voteArgs(link.number, link.remote.org, vote), { key: link.remote.org });
  },

  /** The PR's pushes with their commits: every iteration and its commits in one call, not one call per push. */
  async pushes(link) {
    const out = (await orElse(invoke(link.remote.org, "git", "pullRequestIterations", prRoute(link), ["--query-parameters", "includeCommits=true"]), null)) as { value?: AzureIteration[] } | null;
    return pushesOf(out?.value ?? []);
  },

  async snippet(link, root, pushes, head) {
    if (!root.path || !root.range) return null;
    const push = pushes.find((p) => p.id === root.range!.iteration);
    const sha = root.side === "deletions" ? push?.base : (push?.head ?? head);
    const text = sha ? await fileAt(link, sha, root.path) : null;
    const lines = text === null ? [] : snippetOf(text, root.range.start, root.range.end);
    return lines.length ? lines : null;
  },
};
