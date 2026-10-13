// The token-authenticated forges, Bitbucket Cloud and Gitea/Forgejo: how their API objects map onto the tracker's
// status, rows and comments. No `wings`, no I/O, so node tests drive it.
import { counts } from "./checks.ts";
import type { Comment, FailureDetail, Failing, Icon, Link, ListedPr, Result, Status } from "./types.ts";

/**
 * A value in a query string. `wings.fetch` takes only a URL already in normal form, which leaves `%XX` as written
 * but would rewrite a bare `'`, so everything but the unreserved characters is escaped.
 */
export function encodeQuery(value: string) {
  return encodeURIComponent(value).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
}

/** A failed check has no log to read on these forges, only the summary and a link to the CI's own page. */
export function plainFailure(_link: Link, f: Failing): Promise<FailureDetail[]> {
  return Promise.resolve([{ check: f.name, summary: f.summary ?? "Failed", log: "", url: f.url }]);
}

/** The label, tone and icon every forge's PR goes through; the cases it doesn't cover keep "In review". */
function labelOf(p: { merged?: boolean; closed?: boolean; draft?: boolean; conflicts?: boolean; checks: { failed: number; running: number }; changes?: boolean; approved?: boolean }): [string, Tone, Icon] {
  if (p.merged) return ["Merged", "merged", "pr-merged"];
  if (p.closed) return ["Closed", "neutral", "pr-closed"];
  if (p.draft) return ["Draft", "neutral", "pr-draft"];
  if (p.conflicts) return ["Conflicts", "danger", "pr-open"];
  if (p.checks.failed) return ["Checks failing", "danger", "pr-open"];
  if (p.changes) return ["Changes requested", "danger", "pr-open"];
  if (p.checks.running) return ["Checks running", "warning", "pr-open"];
  if (p.approved) return ["Approved", "success", "pr-open"];
  return ["In review", "info", "pr-open"];
}

const NO_CHECKS = { passed: 0, failed: 0, running: 0 };

// ---------- Bitbucket Cloud ----------

/** The list leaves out each PR's reviewers and participants as too costly, so they are asked for: `+` adds to the default fields and is sent as `%2B` (Bitbucket REST intro, "Partial responses"). */
export const LIST_FIELDS = "fields=%2Bvalues.participants,%2Bvalues.reviewers";


export interface BbUser {
  uuid?: string;
  display_name?: string;
  nickname?: string;
}

/** The fields of a pull request the tracker reads, from `GET /2.0/repositories/{workspace}/{repo}/pullrequests[/{id}]`. */
export interface BbPr {
  id: number;
  title: string;
  description?: string;
  state: "OPEN" | "MERGED" | "DECLINED" | "SUPERSEDED";
  draft?: boolean;
  comment_count?: number;
  author?: BbUser;
  source: { branch: { name: string }; commit?: { hash: string } | null; repository?: { full_name?: string } | null };
  destination: { branch: { name: string } };
  participants?: { role: string; approved?: boolean; state?: string | null; user: BbUser }[];
  reviewers?: BbUser[];
  updated_on: string;
  links: { html: { href: string } };
}

/** A build result from `.../pullrequests/{id}/statuses`. */
export interface BbStatus {
  key: string;
  state: "FAILED" | "INPROGRESS" | "STOPPED" | "SUCCESSFUL";
  name?: string;
  description?: string;
  url?: string;
}

export interface BbComment {
  id: number;
  created_on: string;
  content?: { raw?: string };
  user?: BbUser;
  deleted?: boolean;
  parent?: { id: number } | null;
  inline?: { path: string; from?: number | null; to?: number | null } | null;
  links?: { html?: { href?: string } };
}

const bbName = (u?: BbUser) => u?.display_name ?? u?.nickname ?? "unknown";

function bbResults(statuses: BbStatus[]) {
  const results = statuses.map((s): Result => (s.state === "SUCCESSFUL" ? "passed" : s.state === "FAILED" || s.state === "STOPPED" ? "failed" : "running"));
  const failing = statuses
    .filter((_, i) => results[i] === "failed")
    .map((s): Failing => ({ id: s.key, name: s.name ?? s.key, url: s.url ?? null, summary: s.description || (s.state === "STOPPED" ? "Stopped" : null) }));
  return { checks: counts(results), failing };
}

/** Reviewers who approved, and whether any asked for changes. Bitbucket marks a reviewer `approved`, or `state: "changes_requested"`. */
function bbReviews(pr: BbPr) {
  const reviewers = (pr.participants ?? []).filter((p) => p.role === "REVIEWER");
  return { approved: reviewers.filter((p) => p.approved).length, changes: reviewers.some((p) => p.state === "changes_requested") };
}

export function bbStatusOf(pr: BbPr, statuses: BbStatus[]): Status {
  const { checks, failing } = bbResults(statuses);
  const { approved, changes } = bbReviews(pr);
  const [label, tone, icon] = labelOf({ merged: pr.state === "MERGED", closed: pr.state === "DECLINED" || pr.state === "SUPERSEDED", draft: pr.draft, checks, changes, approved: approved > 0 });
  return {
    // A new push changes the source commit, which is what invalidates the cached diff.
    key: pr.source.commit?.hash ?? pr.updated_on,
    title: pr.title,
    label, tone, icon, checks, failing,
    review: changes ? "Changes requested" : approved ? `${approved} approved` : "No approvals yet",
    changes: null,
    branch: `${pr.source.branch.name} → ${pr.destination.branch.name}`,
    updated: pr.updated_on,
    base: pr.destination.branch.name,
    head: pr.source.branch.name,
  };
}

/** The PRs whose branch is in this repository: a branch name alone also matches a fork's branch of the same name. */
export function bbOwn(prs: BbPr[], fullName: string): BbPr[] {
  return prs.filter((p) => p.source.repository?.full_name?.toLowerCase() === fullName.toLowerCase());
}

/** Who a listed PR is for: written by `me`, or with `me` among its reviewers. */
export function bbWho(pr: BbPr, me: string) {
  const reviewing = (pr.participants ?? []).some((p) => p.role === "REVIEWER" && p.user.uuid === me) || (pr.reviewers ?? []).some((u) => u.uuid === me);
  return { mine: pr.author?.uuid === me, reviewing };
}

/** The open PRs of a repo that are yours or wait for you, in the list's order. `link` builds the ref, since a Bitbucket link carries the folder. */
export function bbListed(prs: BbPr[], me: string, link: (pr: BbPr) => Link): ListedPr[] {
  return prs.flatMap((pr): ListedPr[] => {
    const { mine, reviewing } = bbWho(pr, me);
    if (!mine && !reviewing) return [];
    const { changes } = bbReviews(pr);
    const [label, tone, icon] = labelOf({ draft: pr.draft, checks: NO_CHECKS, changes });
    const status = { ...bbStatusOf(pr, []), label, tone, icon };
    return [{ link: link(pr), title: pr.title, head: pr.source.branch.name, mine, status, activity: pr.comment_count ?? null }];
  });
}

/** Comments in Bitbucket's order. A reply's `parent` is the comment it answers, which may itself be a reply, so each is moved under its thread's first comment. */
export function bbComments(values: BbComment[], url: string): Comment[] {
  const live = values.filter((c) => !c.deleted);
  const parents = new Map(live.map((c) => [c.id, c.parent?.id ?? null]));
  const rootOf = (id: number): number => {
    const parent = parents.get(id);
    return parent === null || parent === undefined || !parents.has(parent) ? id : rootOf(parent);
  };
  return live.map((c): Comment => {
    const root = rootOf(c.id);
    const inline = c.inline;
    return {
      id: c.id,
      replyTo: root === c.id ? null : root,
      path: inline?.path ?? null,
      line: inline ? (inline.to ?? inline.from ?? null) : null,
      side: inline?.to ? "additions" : inline ? "deletions" : "additions",
      author: bbName(c.user),
      body: c.content?.raw ?? "",
      createdAt: c.created_on,
      url: c.links?.html?.href ?? url,
      canReply: root === c.id,
    };
  });
}

// ---------- Gitea and Forgejo ----------

export interface GtUser {
  login?: string;
  full_name?: string;
}

/** The fields of a pull request the tracker reads, from `GET /api/v1/repos/{owner}/{repo}/pulls[/{index}]`. */
export interface GtPr {
  number: number;
  title: string;
  body?: string;
  state: "open" | "closed";
  draft?: boolean;
  merged?: boolean;
  mergeable?: boolean;
  comments?: number;
  review_comments?: number;
  additions?: number;
  deletions?: number;
  changed_files?: number;
  user?: GtUser;
  requested_reviewers?: GtUser[] | null;
  head: { ref: string; sha: string; repo?: { full_name?: string } | null };
  base: { ref: string };
  updated_at: string;
  html_url: string;
}

/** A commit status from `.../commits/{sha}/status`: `success`, `failure`, `error`, `pending`, `warning` or `skipped`. */
export interface GtCommitStatus {
  status: string;
  context?: string;
  description?: string;
  target_url?: string;
}

export interface GtReview {
  id: number;
  user?: GtUser;
  state: string;
  body?: string;
  dismissed?: boolean;
  submitted_at: string;
  html_url?: string;
}

export interface GtComment {
  id: number;
  user?: GtUser;
  body?: string;
  created_at: string;
  html_url?: string;
}

const gtName = (u?: GtUser) => u?.full_name || u?.login || "unknown";

export function gtStatusOf(pr: GtPr, statuses: GtCommitStatus[], reviews: GtReview[]): Status {
  // `warning` doesn't block a merge, and `skipped` didn't run.
  const results = statuses.map((s): Result => (["success", "warning", "skipped"].includes(s.status) ? "passed" : s.status === "pending" ? "running" : "failed"));
  const failing = statuses
    .filter((_, i) => results[i] === "failed")
    .map((s): Failing => ({ id: s.context ?? s.target_url ?? "check", name: s.context, url: s.target_url ?? null, summary: s.description || null }));
  const checks = counts(results);
  // A reviewer's latest verdict counts, and a dismissed one doesn't.
  const latest = new Map<string, string>();
  for (const r of [...reviews].sort((a, b) => a.submitted_at.localeCompare(b.submitted_at))) {
    if (r.dismissed || !["APPROVED", "REQUEST_CHANGES"].includes(r.state)) continue;
    latest.set(r.user?.login ?? String(r.id), r.state);
  }
  const verdicts = [...latest.values()];
  const approved = verdicts.filter((v) => v === "APPROVED").length;
  const changes = verdicts.includes("REQUEST_CHANGES");
  const [label, tone, icon] = labelOf({ merged: pr.merged, closed: pr.state === "closed", draft: pr.draft, conflicts: pr.mergeable === false, checks, changes, approved: approved > 0 });
  const files = pr.changed_files;
  return {
    key: pr.head.sha,
    title: pr.title,
    label, tone, icon, checks, failing,
    review: changes ? "Changes requested" : approved ? `${approved} approved` : "No approvals yet",
    changes: files === undefined ? null : `+${pr.additions ?? 0} −${pr.deletions ?? 0} in ${files} files`,
    branch: `${pr.head.ref} → ${pr.base.ref}`,
    updated: pr.updated_at,
  };
}

/** The PRs whose branch is in this repository: a branch name alone also matches a fork's branch of the same name. */
export function gtOwn(prs: GtPr[], fullName: string): GtPr[] {
  return prs.filter((p) => p.head.repo?.full_name?.toLowerCase() === fullName.toLowerCase());
}

/** Who a listed PR is for: written by `me`, or with `me` among the reviewers asked. */
export function gtWho(pr: GtPr, me: string) {
  const lower = me.toLowerCase();
  return { mine: pr.user?.login?.toLowerCase() === lower, reviewing: (pr.requested_reviewers ?? []).some((u) => u.login?.toLowerCase() === lower) };
}

export function gtListed(prs: GtPr[], me: string, link: (pr: GtPr) => Link): ListedPr[] {
  return prs.flatMap((pr): ListedPr[] => {
    const { mine, reviewing } = gtWho(pr, me);
    if (!mine && !reviewing) return [];
    const [label, tone, icon] = labelOf({ draft: pr.draft, conflicts: pr.mergeable === false, checks: NO_CHECKS });
    return [{ link: link(pr), title: pr.title, head: pr.head.ref, mine, status: { ...gtStatusOf(pr, [], []), label, tone, icon }, activity: (pr.comments ?? 0) + (pr.review_comments ?? 0) }];
  });
}

/**
 * The comments on the whole PR: its conversation and the reviews' summaries. Line comments live under each
 * review (`.../reviews/{id}/comments`) and aren't read, so they show on the forge's own page only. Reviews get
 * negative ids, which can't clash with the conversation's.
 */
export function gtComments(comments: GtComment[], reviews: GtReview[], url: string): Comment[] {
  const general = [
    ...comments.map((c) => ({ id: c.id, user: c.user, body: c.body, at: c.created_at, url: c.html_url })),
    ...reviews.filter((r) => r.body && r.state !== "PENDING").map((r) => ({ id: -r.id, user: r.user, body: r.body, at: r.submitted_at, url: r.html_url })),
  ];
  return general.map((c): Comment => ({ id: c.id, replyTo: null, path: null, line: null, side: "additions", author: gtName(c.user), body: c.body ?? "", createdAt: c.at, url: c.url ?? url, canReply: true }));
}

/**
 * A reply to a Codeberg comment. The API has no threaded replies (a review comment can't take a parent, and the conversation
 * is flat), so a reply is a new comment on the pull request that quotes the first lines of what it answers.
 */
export function gtQuoted(root: Pick<Comment, "author" | "body">, reply: string): string {
  const lines = root.body.trim().split("\n").slice(0, 6).map((l) => `> ${l}`.trimEnd());
  const more = root.body.trim().split("\n").length > 6 ? ["> …"] : [];
  return `> **${root.author}** wrote:\n${[...lines, ...more].join("\n")}\n\n${reply}`;
}
