// The pure half of Azure DevOps update notifications: what counts as an event in the data the REST API returns,
// how mentions are recognised, and how events are de-duplicated and batched. No `wings`, no I/O, so node tests drive it.

export type Kind = "state" | "mention" | "pr-comment" | "pr-mention" | "pr-vote" | "pr-status" | "pr-reviewer";

export interface UpdateEvent {
  /** Stable across polls, so an event seen twice (the watermark overlaps on purpose) is only reported once. */
  key: string;
  kind: Kind;
  title: string;
  body: string;
  url: string;
  /** When it happened on Azure DevOps, ISO 8601. */
  at: string;
  /** Who did it, when the source says: the Updates tab draws their avatar. Events kept before this was recorded have none. */
  by?: string;
  /** The type of the work item it's about, when known. */
  itemType?: string;
  /** The work item it's about, when it's about one: the Updates list opens it in the item tab. */
  item?: { org: string; project: string; id: number };
}

export interface StoredEvent extends UpdateEvent {
  read: boolean;
}

interface Identity {
  id?: string;
  displayName?: string;
}

// ---------- mentions ----------

// Work item comments are HTML, and a mention is `<a href="#" data-vss-mention="version:2.0,<identity id>">@Name</a>`.
const WORK_ITEM_MENTION = /data-vss-mention="version:\d+(?:\.\d+)?,([0-9a-f-]{36})"/gi;
// Pull request comments are plain text, and a mention is `@<identity id>`.
const PR_MENTION = /@<\{?([0-9a-f-]{36})\}?>/gi;

/** True when `text` mentions the identity `me`, in either markup. Exact match on the whole id. */
export function mentionsMe(text: string | undefined | null, me: string): boolean {
  if (!text) return false;
  const id = me.toLowerCase();
  for (const re of [WORK_ITEM_MENTION, PR_MENTION]) {
    for (const m of text.matchAll(re)) if (m[1].toLowerCase() === id) return true;
  }
  return false;
}

/** A comment as one line of plain text: tags dropped, entities decoded, a mention shown as its `@Name`. */
export function plain(text: string, max = 160): string {
  const out = text
    .replace(/@<\{?[0-9a-f-]{36}\}?>/gi, "@you")
    .replace(/<br\s*\/?>|<\/(?:div|p|li)>/gi, " ")
    .replace(/<[^>]*>/g, "")
    .replace(/&nbsp;/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();
  return out.length > max ? `${out.slice(0, max - 1)}…` : out;
}

// ---------- links and time ----------

/** The URL prefix the manifest's `openUrl` allows: a visualstudio.com org also answers at dev.azure.com. */
export function webBase(org: string) {
  return org.replace(/^https:\/\/([^.]+)\.visualstudio\.com$/, "https://dev.azure.com/$1");
}

export const workItemUrl = (org: string, project: string, id: number) => `${webBase(org)}/${encodeURIComponent(project)}/_workitems/edit/${id}`;

export const prUrl = (org: string, project: string, repo: string, id: number, thread?: number) =>
  `${webBase(org)}/${encodeURIComponent(project)}/_git/${encodeURIComponent(repo)}/pullrequest/${id}${thread ? `?_a=overview&discussionId=${thread}` : ""}`;

/** WIQL compares dates by day when it can't take a time, so the query asks from the day before `iso` and the result is filtered by time. */
export function dayBefore(iso: string) {
  return new Date(Date.parse(iso) - 86_400_000).toISOString().slice(0, 10);
}

export const after = (a: string | undefined, b: string) => !!a && Date.parse(a) > Date.parse(b);

/** The watermark to keep after a successful check: the moment it started, less an overlap that clock skew between this computer and Azure can't escape. `seen` keeps the overlap from reporting twice. */
export const watermark = (startedMs: number, overlapMs = 120_000) => new Date(startedMs - overlapMs).toISOString();

// ---------- work items ----------

/** One entry of `GET wit/workItems/{id}/updates`. The newest one's `revisedDate` is 9999-01-01, so the time is its ChangedDate field. */
export interface WorkItemUpdate {
  id: number;
  revisedBy?: Identity;
  revisedDate?: string;
  fields?: Record<string, { oldValue?: unknown; newValue?: unknown } | undefined>;
}

export interface Candidate {
  id: number;
  title: string;
  /** Its work item type, for the Updates tab's icon. */
  type?: string;
  /** Assigned to me or created by me: the items whose state changes are worth a notification. */
  mine: boolean;
}

export function updateTime(u: WorkItemUpdate): string {
  const changed = u.fields?.["System.ChangedDate"]?.newValue;
  return typeof changed === "string" ? changed : (u.revisedDate ?? "");
}

export function workItemEvents(org: string, project: string, item: Candidate, updates: WorkItemUpdate[], me: string, since: string): UpdateEvent[] {
  const events: UpdateEvent[] = [];
  const url = workItemUrl(org, project, item.id);
  const ref = { org, project, id: item.id };
  for (const u of updates) {
    const at = updateTime(u);
    if (!after(at, since) || u.revisedBy?.id?.toLowerCase() === me.toLowerCase()) continue;
    const by = u.revisedBy?.displayName ?? "Someone";
    const state = u.fields?.["System.State"];
    if (item.mine && typeof state?.newValue === "string" && state.newValue !== state.oldValue) {
      const from = typeof state.oldValue === "string" ? `${state.oldValue} → ` : "";
      events.push({ key: `wi:${org}:${item.id}:${u.id}:state`, kind: "state", title: `#${item.id} ${from}${state.newValue}`, body: `${item.title} · ${by}`, url, at, by, itemType: item.type, item: ref });
    }
    const history = u.fields?.["System.History"]?.newValue;
    if (typeof history === "string" && mentionsMe(history, me)) {
      events.push({ key: `wi:${org}:${item.id}:${u.id}:mention`, kind: "mention", title: `${by} mentioned you on #${item.id}`, body: plain(history), url, at, by, itemType: item.type, item: ref });
    }
  }
  return events;
}

// ---------- pull requests ----------

export interface Pr {
  pullRequestId: number;
  title: string;
  status: string;
  createdBy?: Identity;
  reviewers?: (Identity & { vote?: number })[];
  repository: { name: string; id?: string };
}

export interface Thread {
  id: number;
  isDeleted?: boolean;
  comments?: {
    id: number;
    author?: Identity;
    content?: string;
    commentType?: string;
    isDeleted?: boolean;
    publishedDate?: string;
  }[];
}

/** What was last seen of one active pull request. */
export interface PrSnap {
  mine: boolean;
  votes: Record<string, number>;
}

export type PrSnaps = Record<string, PrSnap>;

const same = (a: string | undefined, b: string) => a?.toLowerCase() === b.toLowerCase();

export function snapshot(prs: Pr[], me: string): PrSnaps {
  const out: PrSnaps = {};
  for (const pr of prs) {
    const votes: Record<string, number> = {};
    for (const r of pr.reviewers ?? []) if (r.id) votes[r.id] = r.vote ?? 0;
    out[pr.pullRequestId] = { mine: same(pr.createdBy?.id, me), votes };
  }
  return out;
}

export function voteLabel(vote: number) {
  if (vote >= 10) return "approved";
  if (vote >= 5) return "approved with suggestions";
  if (vote <= -10) return "rejected";
  if (vote <= -5) return "is waiting for the author";
  return "reset their vote";
}

export interface PrDiff {
  events: UpdateEvent[];
  /** Pull requests of mine that were active last time and aren't now: completed or abandoned, to be looked up. */
  gone: number[];
}

/** Compares the active pull requests with the last snapshot: me added as a reviewer, a reviewer's vote on my pull request, and which of mine left the active list. A pull request not in the snapshot has nothing to compare with except being new to me. */
export function diffPrs(org: string, project: string, prev: PrSnaps, prs: Pr[], me: string, at: string): PrDiff {
  const events: UpdateEvent[] = [];
  for (const pr of prs) {
    const before = prev[pr.pullRequestId];
    const url = prUrl(org, project, pr.repository.name, pr.pullRequestId);
    const mine = same(pr.createdBy?.id, me);
    if (!before) {
      if (!mine) {
        events.push({ key: `pr:${org}:${pr.pullRequestId}:reviewer`, kind: "pr-reviewer", title: `${pr.createdBy?.displayName ?? "Someone"} asked you to review PR #${pr.pullRequestId}`, body: pr.title, url, at, by: pr.createdBy?.displayName });
      }
      continue;
    }
    if (!mine) continue;
    for (const r of pr.reviewers ?? []) {
      if (!r.id || same(r.id, me)) continue;
      const vote = r.vote ?? 0;
      if (vote === (before.votes[r.id] ?? 0)) continue;
      events.push({ key: `pr:${org}:${pr.pullRequestId}:vote:${r.id}:${vote}`, kind: "pr-vote", title: `${r.displayName ?? "A reviewer"} ${voteLabel(vote)} on PR #${pr.pullRequestId}`, body: pr.title, url, at, by: r.displayName });
    }
  }
  const active = new Set(prs.map((p) => String(p.pullRequestId)));
  const gone = Object.entries(prev)
    .filter(([id, snap]) => snap.mine && !active.has(id))
    .map(([id]) => Number(id));
  return { events, gone };
}

/** The event for one of my pull requests that was looked up after leaving the active list. */
export function statusEvent(org: string, project: string, pr: Pr, at: string): UpdateEvent | null {
  if (pr.status !== "completed" && pr.status !== "abandoned") return null;
  return { key: `pr:${org}:${pr.pullRequestId}:status:${pr.status}`, kind: "pr-status", title: `PR #${pr.pullRequestId} was ${pr.status}`, body: pr.title, url: prUrl(org, project, pr.repository.name, pr.pullRequestId), at };
}

/**
 * New comments on one active pull request, by someone else. On a pull request of mine every one counts; on one I
 * review, only a comment that mentions me or lands in a thread I've written in, since all of them would be noise.
 */
export function threadEvents(org: string, project: string, pr: Pr, threads: Thread[], me: string, since: string): UpdateEvent[] {
  const events: UpdateEvent[] = [];
  const mine = same(pr.createdBy?.id, me);
  for (const thread of threads) {
    if (thread.isDeleted) continue;
    const comments = (thread.comments ?? []).filter((c) => !c.isDeleted && c.commentType !== "system");
    const involved = comments.some((c) => same(c.author?.id, me));
    for (const c of comments) {
      if (!after(c.publishedDate, since) || !c.publishedDate || !c.author?.id || same(c.author.id, me)) continue;
      const mention = mentionsMe(c.content, me);
      if (!mention && !mine && !involved) continue;
      const by = c.author.displayName ?? "Someone";
      events.push({
        key: `pr:${org}:${pr.pullRequestId}:${thread.id}:${c.id}`,
        kind: mention ? "pr-mention" : "pr-comment",
        title: mention ? `${by} mentioned you on PR #${pr.pullRequestId}` : `${by} commented on PR #${pr.pullRequestId}`,
        body: plain(c.content ?? ""),
        url: prUrl(org, project, pr.repository.name, pr.pullRequestId, thread.id),
        at: c.publishedDate,
        by,
      });
    }
  }
  return events;
}

// ---------- seen, batching, backoff ----------

export type Seen = Record<string, string>;

/** The events not reported before. */
export const fresh = (events: UpdateEvent[], seen: Seen) => events.filter((e, i) => !seen[e.key] && events.findIndex((o) => o.key === e.key) === i);

/** Remembers `events` in `seen`, keeping the newest `cap`. */
export function remember(seen: Seen, events: UpdateEvent[], cap = 500): Seen {
  const all = { ...seen, ...Object.fromEntries(events.map((e) => [e.key, e.at])) };
  return Object.fromEntries(Object.entries(all).sort((a, b) => Date.parse(b[1]) - Date.parse(a[1])).slice(0, cap));
}

export interface Note {
  title: string;
  body: string;
}

/** Wings allows 3 notifications a minute per plugin: up to `max` events go one each, more become one summary. */
export function notes(events: UpdateEvent[], max = 3): Note[] {
  if (events.length === 0) return [];
  if (events.length <= max) return events.map((e) => ({ title: e.title, body: e.body }));
  const shown = events.slice(0, 3).map((e) => e.title);
  return [{ title: `${events.length} Azure DevOps updates`, body: `${shown.join("; ")}; and ${events.length - shown.length} more` }];
}

/** How long to leave an org alone after `failures` failed checks in a row: doubles the interval, up to 30 minutes. */
export const backoffMs = (failures: number, intervalMs: number, maxMs = 30 * 60_000) => Math.min(intervalMs * 2 ** Math.max(0, failures - 1), maxMs);

/** `project` moved to the front of an org's known projects, at most `cap` kept. */
export function rememberProject(known: Record<string, string[]>, org: string, project: string, cap = 8): Record<string, string[]> {
  return { ...known, [org]: [project, ...(known[org] ?? []).filter((p) => p !== project)].slice(0, cap) };
}
