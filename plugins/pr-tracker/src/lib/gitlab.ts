/** GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`. */
import { cleanLog, counts, detail } from "./checks";
import { cli, post } from "./cli";
import { problems } from "./state";
import type { Comment, FailureDetail, Failing, GitlabLink, GitlabRemote, Icon, Link, ListedPr, Result, Reviewer, Status } from "./types";

/** The fields of a merge request the tracker reads. */
interface GlMr {
  iid: number;
  web_url: string;
  state: string;
  title: string;
  description?: string | null;
  author?: { name?: string; username?: string };
  user_notes_count?: number;
  created_at?: string;
  draft?: boolean;
  has_conflicts?: boolean;
  detailed_merge_status?: string;
  head_pipeline?: { id: number; project_id: number } | null;
  sha: string;
  changes_count?: string | null;
  source_branch: string;
  target_branch: string;
  updated_at: string;
}

interface GlJob {
  id: number;
  name: string;
  status: string;
  allow_failure?: boolean;
  web_url: string;
}

interface GlNote {
  id: number;
  system?: boolean;
  type?: string | null;
  resolvable?: boolean;
  resolved?: boolean;
  position?: { new_path?: string; old_path?: string; new_line?: number | null; old_line?: number | null };
  author?: { name?: string; username?: string };
  body?: string;
  created_at: string;
}

// ---------- finding the PR for a pane ----------

/** `glab api` against the remote's host. Returns parsed JSON (or the text, with `raw`) or throws, noting a signed-out or missing `glab`. */
async function glab(remote: GitlabRemote, endpoint: string, { paginate = false, raw = false } = {}): Promise<unknown> {
  const key = `gitlab:${remote.host}`;
  let out;
  try {
    out = await cli(key, "glab", ["api", "--hostname", remote.host, ...(paginate ? ["--paginate"] : []), endpoint]);
  } catch (error) {
    if (!problems.get(key)?.missing) problems.set(key, { login: false, message: String((error as Error).message ?? error) });
    throw error;
  }
  if (out.code === 0) return raw ? out.stdout : JSON.parse(out.stdout);
  // A private project looks missing (404) to a signed-out user, so ask glab whether it's signed in.
  const signedOut =
    /\b401\b/.test(out.stderr) || (/\b404\b/.test(out.stderr) && (await wings.exec("glab", ["auth", "status", "--hostname", remote.host])).code !== 0);
  if (signedOut) problems.set(key, { login: true, message: "Sign in to GitLab to see this branch's merge request." });
  throw new Error(out.stderr.trim() || "glab failed");
}

export const project = (remote: GitlabRemote) => `projects/${encodeURIComponent(remote.path)}`;

export async function findGitlab(_cwd: string, remote: GitlabRemote, branch: string): Promise<Link | null> {
  const mrs = (await glab(remote, `${project(remote)}/merge_requests?source_branch=${encodeURIComponent(branch)}&state=all&order_by=updated_at&per_page=5`)) as GlMr[];
  // The open MR wins; otherwise the newest one for this branch.
  const mr = mrs.find((m) => m.state === "opened") ?? mrs[0];
  if (!mr) return null;
  return { provider: "gitlab", url: mr.web_url, number: mr.iid, repo: remote.path, remote };
}

// ---------- status ----------

/** GitLab status from the MR, its approvals and its head pipeline's jobs. */
export async function gitlabStatus(link: GitlabLink): Promise<Status> {
  return gitlabStatusOf(link, await glabMr(link));
}

const glabMr = (link: GitlabLink) => glab(link.remote, `${project(link.remote)}/merge_requests/${link.number}`) as Promise<GlMr>;

/** The status page: the status plus who wrote the MR and its description. */
export async function gitlabOverview(link: GitlabLink) {
  const mr = await glabMr(link);
  const approved = (await glab(link.remote, `${project(link.remote)}/merge_requests/${link.number}/approvals`).catch(() => null)) as { approved_by?: { user?: { name?: string; username?: string } }[] } | null;
  const reviewers = (approved?.approved_by ?? []).map((a): Reviewer => ({ name: a.user?.name ?? a.user?.username ?? "unknown", vote: "approved" }));
  return { status: await gitlabStatusOf(link, mr), author: mr.author?.name ?? mr.author?.username ?? "unknown", body: mr.description ?? "", createdAt: mr.created_at, reviewers };
}

async function gitlabStatusOf(link: GitlabLink, mr: GlMr): Promise<Status> {
  const base = `${project(link.remote)}/merge_requests/${link.number}`;
  const pipeline = mr.head_pipeline;
  const [approvals, jobs] = (await Promise.all([
    glab(link.remote, `${base}/approvals`).catch(() => null),
    pipeline ? glab(link.remote, `projects/${pipeline.project_id}/pipelines/${pipeline.id}/jobs?per_page=100`, { paginate: true }).catch(() => []) : [],
  ])) as [{ approved?: boolean; approved_by?: unknown[]; approvals_left?: number } | null, GlJob[]];
  // Manual jobs haven't run and don't block, so they don't count. A failure that's allowed to fail passes.
  const counted = jobs.filter((j) => j.status !== "manual");
  const results = counted.map(
    (j): Result =>
      j.status === "success" || j.status === "skipped" || (j.status === "failed" && j.allow_failure)
        ? "passed"
        : j.status === "failed" || j.status === "canceled"
          ? "failed"
          : "running",
  );
  const checks = counts(results);
  const failing = counted
    .filter((_, i) => results[i] === "failed")
    .map((j): Failing => ({ id: j.id, name: j.name, job: j.id, project: pipeline!.project_id, url: j.web_url, summary: j.status === "canceled" ? "Canceled" : null }));
  const merge = mr.detailed_merge_status;
  let [label, tone, icon]: [string, Tone, Icon] = ["In review", "info", "pr-open"];
  if (mr.state === "merged") [label, tone, icon] = ["Merged", "merged", "pr-merged"];
  else if (mr.state === "closed" || mr.state === "locked") [label, tone, icon] = ["Closed", "neutral", "pr-closed"];
  else if (mr.draft) [label, tone, icon] = ["Draft", "neutral", "pr-draft"];
  else if (mr.has_conflicts || merge === "conflict") [label, tone] = ["Conflicts", "danger"];
  else if (checks.failed) [label, tone] = ["Checks failing", "danger"];
  else if (merge === "requested_changes") [label, tone] = ["Changes requested", "danger"];
  else if (checks.running) [label, tone] = ["Checks running", "warning"];
  else if (merge === "discussions_not_resolved") [label, tone] = ["Unresolved threads", "warning"];
  else if (approvals?.approved) [label, tone] = ["Approved", "success"];
  const approvedBy = approvals?.approved_by?.length ?? 0;
  const left = approvals?.approvals_left ?? 0;
  const review = [approvedBy && `${approvedBy} approved`, left && `${left} more needed`].filter(Boolean).join(", ") || "No approvals yet";
  return {
    // A new push changes the head commit, which is what invalidates the cached diff.
    key: mr.sha,
    title: mr.title,
    label, tone, icon, checks, review, failing,
    changes: mr.changes_count ? `${mr.changes_count} files` : null,
    branch: `${mr.source_branch} → ${mr.target_branch}`,
    updated: mr.updated_at,
  };
}

// ---------- the project's open MRs ----------

/** What the list can say without a call per MR: the merge status stands in for the pipeline's jobs. */
function listStatus(mr: GlMr): Status {
  const merge = mr.detailed_merge_status;
  let [label, tone, icon]: [string, Tone, Icon] = ["In review", "info", "pr-open"];
  if (mr.draft) [label, tone, icon] = ["Draft", "neutral", "pr-draft"];
  else if (mr.has_conflicts || merge === "conflict") [label, tone] = ["Conflicts", "danger"];
  else if (merge === "ci_must_pass") [label, tone] = ["Checks not passing", "warning"];
  else if (merge === "requested_changes") [label, tone] = ["Changes requested", "danger"];
  else if (merge === "ci_still_running") [label, tone] = ["Checks running", "warning"];
  else if (merge === "discussions_not_resolved") [label, tone] = ["Unresolved threads", "warning"];
  else if (merge === "mergeable") [label, tone] = ["Ready to merge", "success"];
  return {
    key: mr.sha,
    title: mr.title,
    label, tone, icon,
    checks: { passed: 0, failed: 0, running: 0 },
    review: "",
    failing: [],
    changes: null,
    branch: `${mr.source_branch} → ${mr.target_branch}`,
    updated: mr.updated_at,
  };
}

/** One `glab api` per query: the open MRs you wrote (`created_by_me`) and the ones you review (`reviews_for_me`). */
export async function gitlabList(remote: GitlabRemote): Promise<ListedPr[]> {
  const query = async (scope: string, mine: boolean) =>
    ((await glab(remote, `${project(remote)}/merge_requests?state=opened&scope=${scope}&order_by=updated_at&per_page=50`)) as GlMr[]).map(
      (mr): ListedPr => ({
        link: { provider: "gitlab", url: mr.web_url, number: mr.iid, repo: remote.path, remote },
        title: mr.title,
        head: mr.source_branch,
        mine,
        status: listStatus(mr),
        activity: mr.user_notes_count ?? null,
      }),
    );
  const [mine, review] = await Promise.all([query("created_by_me", true), query("reviews_for_me", false)]);
  return [...mine, ...review.filter((r) => !mine.some((m) => m.link.url === r.link.url))];
}

// ---------- failed checks ----------

/** A failed GitLab job: its trace. */
export async function gitlabFailure(link: GitlabLink, f: Failing): Promise<FailureDetail[]> {
  if (f.summary) return [{ check: f.name, summary: f.summary, log: "", url: f.url }];
  const trace = (await glab(link.remote, `projects/${f.project}/jobs/${f.job}/trace`, { raw: true })) as string;
  return [detail(f.name, null, cleanLog(trace.split("\n")), [], f.url)];
}

// ---------- diff and comments ----------

export async function gitlabDiff(link: GitlabLink) {
  const [diff, comments] = await Promise.all([
    cli(`gitlab:${link.remote.host}`, "glab", ["mr", "diff", String(link.number), "--raw", "--repo", `https://${link.remote.host}/${link.remote.path}`]),
    gitlabComments(link),
  ]);
  if (diff.code !== 0) throw new Error(diff.stderr.trim() || "glab mr diff failed");
  return { patch: diff.stdout, comments };
}

/** The MR's discussions, without the diff. `system` adds GitLab's own notes ("added 2 commits"), marked as such. */
export async function gitlabComments(link: GitlabLink, system = false): Promise<Comment[]> {
  const discussions = (await glab(link.remote, `${project(link.remote)}/merge_requests/${link.number}/discussions?per_page=100`, { paginate: true }).catch(() => [])) as {
    id: string;
    notes?: GlNote[];
  }[];
  const comments: Comment[] = [];
  for (const d of discussions) {
    const notes = (d.notes ?? []).filter((n) => system || !n.system);
    notes.forEach((n, i) => {
      // Only diff notes have a position; the rest are comments on the whole MR.
      const pos = n.type === "DiffNote" ? n.position : null;
      comments.push({
        id: n.id,
        replyTo: i === 0 ? null : notes[0].id,
        path: pos ? (pos.new_path ?? pos.old_path)! : null,
        line: pos ? (pos.new_line ?? pos.old_line ?? null) : null,
        side: pos?.new_line ? "additions" : "deletions",
        author: n.author?.name ?? n.author?.username ?? "unknown",
        body: n.body ?? "",
        createdAt: n.created_at,
        url: link.url,
        canReply: i === 0 && !n.system,
        discussion: d.id,
        ...(i === 0 && n.resolvable ? { thread: n.resolved ? { state: "resolved" as const, label: "Resolved" } : { state: "active" as const, label: "Unresolved" } } : {}),
        ...(n.system ? { system: true } : {}),
      });
    });
  }
  return comments;
}

// ---------- replies ----------

export async function gitlabReply(link: GitlabLink, root: Comment, body: string): Promise<Comment> {
  const n = (await post("glab", `${project(link.remote)}/merge_requests/${link.number}/discussions/${root.discussion}/notes`, { body }, { host: link.remote.host })) as GlNote;
  return { ...root, id: n.id, replyTo: root.id, author: n.author?.name ?? n.author?.username ?? "you", body: n.body ?? body, createdAt: n.created_at, canReply: false };
}
