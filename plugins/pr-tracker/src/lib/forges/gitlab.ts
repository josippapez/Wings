/** GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`. */
import { cached, errorMessage } from "wings-plugin-sdk/async";
import { cli, CliError } from "wings-plugin-sdk/cli";
import { real } from "../checklist";
import { cleanLog, detail } from "../logs";
import { post } from "./post";
import { counts, labelOf, ladder, noLog, openOrNewest, same } from "./shared";
import { AccountProblem, type Forge } from "./types";
import type { CheckItem, CheckState, Comment, Failing, GitlabLink, GitlabRemote, ListedPr, Result, Reviewer, Status } from "../types";

const keyOf = (host: string) => `gitlab:${host}`;
const hostOfKey = (key: string | undefined) => (key ?? "").slice("gitlab:".length);

export const glab = cli("glab", {
  // Public projects answer some calls without sign-in, so a problem stays until Try again or a sign-in, or the badge would flip back and forth.
  sticky: true,
  // A private project looks missing (404) to a signed-out user, so ask glab whether it's signed in.
  signedOut: async ({ stderr }, key) => /\b401\b/.test(stderr) || (/\b404\b/.test(stderr) && (await wings.exec("glab", ["auth", "status", "--hostname", hostOfKey(key)])).code !== 0),
  signIn: (key) => ["auth", "login", "--web", "--hostname", hostOfKey(key)],
});

// ---------- the API's shapes and how they map ----------

/** The fields of a merge request the tracker reads. */
export interface GlMr {
  iid: number;
  web_url: string;
  state: string;
  title: string;
  description?: string | null;
  author?: { name?: string; username?: string };
  reviewers?: { username?: string }[];
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

export interface GlJob {
  id: number;
  name: string;
  status: string;
  stage?: string;
  allow_failure?: boolean;
  web_url: string;
  started_at?: string | null;
  finished_at?: string | null;
  duration?: number | null;
}

export interface GlApprovals {
  approved?: boolean;
  approved_by?: { user?: { name?: string; username?: string } }[];
  approvals_left?: number;
}

export interface GlNote {
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

const GL_QUEUED = ["created", "pending", "scheduled", "waiting_for_resource", "waiting_for_callback"];

/** A pipeline job as a check of the Checks view. */
export function glItem(j: GlJob, projectId: number | undefined): CheckItem {
  let state: CheckState;
  let note: string | null = null;
  if (j.status === "success") state = "passed";
  else if (j.status === "skipped") [state, note] = ["skipped", "Skipped"];
  else if (j.status === "manual") [state, note] = ["skipped", "Manual job: start it in GitLab"];
  else if (j.status === "failed" && j.allow_failure) [state, note] = ["passed", "Failed, but allowed to fail"];
  else if (j.status === "failed") state = "failed";
  else if (j.status === "canceled" || j.status === "canceling") [state, note] = ["failed", "Canceled"];
  else state = GL_QUEUED.includes(j.status) ? "queued" : "running";
  return {
    id: String(j.id),
    name: j.name,
    state,
    required: j.allow_failure ? false : true,
    kind: j.stage ? `Job in ${j.stage}` : "Job",
    at: real(j.finished_at) ?? real(j.started_at),
    seconds: j.duration ?? null,
    url: j.web_url,
    note,
    failing: state === "failed" ? { id: j.id, name: j.name, job: j.id, project: projectId, url: j.web_url, summary: j.status === "canceled" ? "Canceled" : null } : undefined,
  };
}

/** GitLab status from the MR, its approvals and its head pipeline's jobs. */
export function glStatusOf(mr: GlMr, approvals: GlApprovals | null, jobs: GlJob[]): Status {
  const project = mr.head_pipeline?.project_id;
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
    .map((j): Failing => ({ id: j.id, name: j.name, job: j.id, project, url: j.web_url, summary: j.status === "canceled" ? "Canceled" : null }));
  const merge = mr.detailed_merge_status;
  const [label, tone, icon] = labelOf({
    merged: mr.state === "merged",
    closed: mr.state === "closed" || mr.state === "locked",
    draft: mr.draft,
    conflicts: mr.has_conflicts || merge === "conflict",
    checks,
    changes: merge === "requested_changes",
    unresolved: merge === "discussions_not_resolved",
    approved: approvals?.approved,
  });
  const approvedBy = approvals?.approved_by?.length ?? 0;
  const left = approvals?.approvals_left ?? 0;
  const review = [approvedBy && `${approvedBy} approved`, left && `${left} more needed`].filter(Boolean).join(", ") || "No approvals yet";
  return {
    // A new push changes the head commit, which is what invalidates the cached diff.
    key: mr.sha,
    title: mr.title,
    label, tone, icon, checks, review, failing,
    // Manual jobs are listed, though they don't count.
    items: jobs.map((j) => glItem(j, project)),
    changes: mr.changes_count ? `${mr.changes_count} files` : null,
    branch: `${mr.source_branch} → ${mr.target_branch}`,
    updated: mr.updated_at,
  };
}

/** What the list can say without a call per MR: the merge status stands in for the pipeline's jobs. */
export function glListStatus(mr: GlMr): Status {
  const merge = mr.detailed_merge_status;
  const [label, tone, icon] = ladder([
    [mr.draft, "Draft", "neutral", "pr-draft"],
    [mr.has_conflicts || merge === "conflict", "Conflicts", "danger"],
    [merge === "ci_must_pass", "Checks not passing", "warning"],
    [merge === "requested_changes", "Changes requested", "danger"],
    [merge === "ci_still_running", "Checks running", "warning"],
    [merge === "discussions_not_resolved", "Unresolved threads", "warning"],
    [merge === "mergeable", "Ready to merge", "success"],
  ]);
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

const nameOf = (u: { name?: string; username?: string } | undefined) => u?.name ?? u?.username;

/** Whose a listed MR is. `me` is the username `glab api user` gives. */
export function glWho(mr: Pick<GlMr, "author" | "reviewers">, me: string) {
  return { mine: same(mr.author?.username, me), reviewing: (mr.reviewers ?? []).some((r) => same(r.username, me)) };
}

/** Every open MR of the project, each marked yours or waiting for you. */
export function glListed(mrs: GlMr[], me: string, toLink: (mr: GlMr) => GitlabLink): ListedPr[] {
  return mrs.map((mr): ListedPr => ({
    link: toLink(mr),
    title: mr.title,
    head: mr.source_branch,
    ...glWho(mr, me),
    author: nameOf(mr.author) ?? null,
    status: glListStatus(mr),
    loaded: false,
    activity: mr.user_notes_count ?? null,
  }));
}

export function glReviewers(approvals: GlApprovals | null): Reviewer[] {
  return (approvals?.approved_by ?? []).map((a): Reviewer => ({ name: nameOf(a.user) ?? "unknown", vote: "approved" }));
}

/** The MR's discussions as comments. `system` adds GitLab's own notes ("added 2 commits"), marked as such. */
export function glComments(discussions: { id: string; notes?: GlNote[] }[], url: string, system = false): Comment[] {
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
        author: nameOf(n.author) ?? "unknown",
        body: n.body ?? "",
        createdAt: n.created_at,
        url,
        canReply: i === 0 && !n.system,
        discussion: d.id,
        ...(i === 0 && n.resolvable ? { thread: n.resolved ? { state: "resolved" as const, label: "Resolved" } : { state: "active" as const, label: "Unresolved" } } : {}),
        ...(n.system ? { system: true } : {}),
      });
    });
  }
  return comments;
}

/** The comment a reply made, in the thread of `root`. */
export function glReplied(root: Comment, note: GlNote, body: string): Comment {
  return { ...root, id: note.id, replyTo: root.id, author: nameOf(note.author) ?? "you", body: note.body ?? body, createdAt: note.created_at, canReply: false };
}

// ---------- calling it ----------

/** A call whose program never ran (it printed nothing to stderr, as a program that didn't start does) can't say which MR a branch has: the pane says PRs are unavailable. A command that ran and failed is just an error. */
async function ran<T>(call: Promise<T>): Promise<T> {
  try {
    return await call;
  } catch (error) {
    if (error instanceof CliError && error.kind === "failed" && !error.stderr) throw new AccountProblem("unavailable", errorMessage(error));
    throw error;
  }
}

const apiArgs = (remote: GitlabRemote, endpoint: string, paginate = false) => ["api", "--hostname", remote.host, ...(paginate ? ["--paginate"] : []), endpoint];

/** `glab api` against the remote's host, which prints JSON. */
async function api<T>(remote: GitlabRemote, endpoint: string, paginate = false): Promise<T> {
  const value = await ran(glab.json<T>(apiArgs(remote, endpoint, paginate), { key: keyOf(remote.host) }));
  if (value === null) throw new Error("glab api printed nothing");
  return value;
}

const project = (remote: GitlabRemote) => `projects/${encodeURIComponent(remote.path)}`;
const mrPath = (link: GitlabLink) => `${project(link.remote)}/merge_requests/${link.number}`;
const toLink = (mr: GlMr, remote: GitlabRemote): GitlabLink => ({ provider: "gitlab", url: mr.web_url, number: mr.iid, repo: remote.path, remote });

/** The username `glab` is signed in as, per host. */
const users = cached(async (remote: GitlabRemote) => {
  const name = (await api<{ username?: string }>(remote, "user")).username;
  if (!name) throw new Error("GitLab didn't say who you are.");
  return name;
}, { key: (remote) => remote.host });

/** The MR with what a status is made of: its approvals and its head pipeline's jobs, which a failure of either leaves out. */
async function load(link: GitlabLink) {
  const mr = await api<GlMr>(link.remote, mrPath(link));
  const pipeline = mr.head_pipeline;
  const [approvals, jobs] = await Promise.all([
    api<GlApprovals>(link.remote, `${mrPath(link)}/approvals`).catch(() => null),
    pipeline ? api<GlJob[]>(link.remote, `projects/${pipeline.project_id}/pipelines/${pipeline.id}/jobs?per_page=100`, true).catch((): GlJob[] => []) : [],
  ]);
  return { mr, approvals, jobs };
}

async function comments(link: GitlabLink, system: boolean): Promise<Comment[]> {
  const discussions = await api<{ id: string; notes?: GlNote[] }[]>(link.remote, `${mrPath(link)}/discussions?per_page=100`, true).catch(() => []);
  return glComments(discussions, link.url, system);
}

export const gitlab: Forge<GitlabLink, GitlabRemote> = {
  problemKey: (remote) => keyOf(remote.host),
  repoKey: (remote) => `gitlab:${remote.host}/${remote.path}`,

  login: {
    cli: glab,
    words: () => ({
      signedOut: "Sign in to GitLab to see this branch's merge request.",
      missing: "PR tracker needs the GitLab CLI (glab) to find this branch's pull request.",
    }),
    signIn: (remote) => glab.signIn(keyOf(remote.host)),
  },

  async find(_cwd, remote, branch) {
    const mrs = await api<GlMr[]>(remote, `${project(remote)}/merge_requests?source_branch=${encodeURIComponent(branch)}&state=all&order_by=updated_at&per_page=5`);
    const mr = openOrNewest(mrs, (m) => m.state === "opened");
    return mr ? toLink(mr, remote) : null;
  },

  /** Every open MR of the project (up to 100) in one call, each marked yours or waiting for you. */
  async list(_cwd, remote) {
    const [user, mrs] = await Promise.all([users.get(remote), api<GlMr[]>(remote, `${project(remote)}/merge_requests?state=opened&order_by=updated_at&per_page=100`)]);
    return glListed(mrs, user, (mr) => toLink(mr, remote));
  },

  async linkOf({ remote, url, number, repo }) {
    if (remote?.kind !== "gitlab") throw new Error("This tab doesn't say which GitLab project its merge request is in.");
    return { provider: "gitlab", url, number, repo, remote };
  },

  async status(link) {
    const { mr, approvals, jobs } = await load(link);
    return glStatusOf(mr, approvals, jobs);
  },

  async overview(link) {
    const { mr, approvals, jobs } = await load(link);
    return { status: glStatusOf(mr, approvals, jobs), author: nameOf(mr.author) ?? "unknown", body: mr.description ?? "", createdAt: mr.created_at, reviewers: glReviewers(approvals) };
  },

  async diff(link) {
    const [patch, conversation] = await Promise.all([
      glab.run(["mr", "diff", String(link.number), "--raw", "--repo", `https://${link.remote.host}/${link.remote.path}`], { key: keyOf(link.remote.host) }),
      comments(link, false),
    ]);
    return { patch, comments: conversation };
  },

  /** The MR's discussions with GitLab's own notes, which the diff viewer leaves out. */
  comments: (link) => comments(link, true),

  /** A failed GitLab job: its trace. */
  async failure(link, f) {
    if (f.summary) return noLog(f.name, f.summary, f.url);
    const trace = await ran(glab.run(apiArgs(link.remote, `projects/${f.project}/jobs/${f.job}/trace`), { key: keyOf(link.remote.host) }));
    return [detail(f.name, null, cleanLog(trace.split("\n")), [], f.url)];
  },

  async reply(link, root, body) {
    const note = (await post("glab", `${mrPath(link)}/discussions/${root.discussion}/notes`, { body }, { host: link.remote.host })) as GlNote;
    return glReplied(root, note, body);
  },

  /**
   * Approves the head commit you loaded (`sha`, the status's key), not whatever the head is now: GitLab answers 409 when
   * the branch has moved on, so you never approve commits you haven't seen.
   */
  async approve(link, sha) {
    try {
      await post("glab", `${mrPath(link)}/approve`, { sha }, { host: link.remote.host });
    } catch (e) {
      if (/\b409\b|conflict|does not match/i.test(errorMessage(e))) throw new Error("New commits since you loaded this PR. Refresh and review again.");
      throw e;
    }
  },

  forget: () => users.clear(),
};
