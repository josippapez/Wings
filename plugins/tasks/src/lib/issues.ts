// Issues live in wings.storage, one entry per project id (`issues:<projectId>`). Only the main script writes them:
// pages send it small operations and the tools call the same ones, so a page and an agent never overwrite each other.
// Everything here is plain logic with no calls into Wings except `readStore`, `writeStore` and `migrateLegacy`.

/**
 * Why these numbers: all of the plugin's storage is 1 MB, shared by every project. An issue with a short description and
 * ten one-line log entries is about 2 KB of JSON (see `estimateBytes` and the test that measures it), so 300 of them are
 * about 600 KB. `BUDGET` stops one project well before the 1 MB wall, with a message instead of storage's own refusal.
 * A broadcast is limited to 64,000 characters, and none carries issue text from main to a page: it carries ids.
 */
export const LIMITS = { issues: 300, title: 200, description: 20_000, note: 4_000, branch: 200, pr: 500, log: 300 } as const;
/** The most one project's entry may take, in bytes of JSON. */
export const BUDGET = 700_000;
export const FORMAT_VERSION = 3;
/** Links one issue keeps to others, of each kind. */
export const MAX_LINKS = 20;

export type Status = "todo" | "in_progress" | "in_review" | "done" | "canceled";
export const STATUSES: Status[] = ["todo", "in_progress", "in_review", "done", "canceled"];
/** The columns of the board, in the order an issue moves through them. Canceled sits behind a toggle. */
export const FLOW: Status[] = ["todo", "in_progress", "in_review", "done"];
export const STATUS_LABEL: Record<Status, string> = { todo: "To do", in_progress: "In progress", in_review: "In review", done: "Done", canceled: "Canceled" };

export type IssueType = "task" | "bug" | "feature" | "epic";
export const TYPES: IssueType[] = ["task", "bug", "feature", "epic"];
export const TYPE_LABEL: Record<IssueType, string> = { task: "Task", bug: "Bug", feature: "Feature", epic: "Epic" };
/** Only these can be somebody's parent. */
export const canParent = (type: IssueType): boolean => type === "epic" || type === "feature";

/**
 * Who wrote a log entry: the user in the UI, "git" for what the plugin found in the repository by itself (a commit or
 * branch with the issue's key), or the Claude session an agent's tool call came from. "git" is its own kind rather than
 * "you" so the log never claims the user did something they didn't do.
 */
export type Actor = "you" | "git" | { id: string; name?: string };
export type LogKind = "created" | "status" | "note" | "link" | "type";
export interface LogEntry {
  at: number;
  by: Actor;
  kind: LogKind;
  text: string;
}
export interface SessionRef {
  id: string;
  name?: string;
  /** When a tool call from this session last touched the issue. */
  seen: number;
}
export interface Issue {
  /** Like `WIN-12`. Never reused. */
  key: string;
  title: string;
  /** Markdown: goal, scope, acceptance criteria. */
  description?: string;
  status: Status;
  type: IssueType;
  /** The epic or feature this belongs to. Same project, never itself or one of its own children. */
  parent?: string;
  /** Symmetric: if A lists B, B lists A. */
  related?: string[];
  /** The issues that have to finish first. Their `blocks` is worked out from this, not stored. */
  blockedBy?: string[];
  links?: { branch?: string; pr?: string; commits?: string[] };
  /** Append-only and deduped by id. */
  sessions: SessionRef[];
  /** Append-only. Nothing edits or removes an entry. */
  log: LogEntry[];
  createdAt: number;
  updatedAt: number;
}
/** What one project's storage entry holds. `next` is the number the next issue gets, and only ever grows. */
export interface Store {
  v: typeof FORMAT_VERSION;
  prefix: string;
  next: number;
  issues: Issue[];
}

/** What a page or a tool asks main to do. For `update`, `branch` and `pr` "" clear the link. */
export interface Relations {
  type?: IssueType;
  /** A key, or "" to clear. */
  parent?: string;
  relatedAdd?: string[];
  relatedRemove?: string[];
  blockedByAdd?: string[];
  blockedByRemove?: string[];
}
export type Op =
  | ({ kind: "create"; title: string; description?: string; status?: Status; branch?: string } & Relations)
  | ({ kind: "update"; key: string; title?: string; description?: string; status?: Status; branch?: string; pr?: string; commit?: string } & Relations)
  | { kind: "note"; key: string; text: string }
  | { kind: "remove"; key: string };

/** A page asks main to apply `op` to a project's issues; `rid` names the request. `projectName` seeds the key prefix. */
export interface OpMessage {
  type: "op";
  projectId: string;
  projectName: string;
  rid: string;
  op: Op;
}
/** Main tells every page that a project's issues changed, so they read them again. `error` answers a failed `rid`. */
export interface IssuesMessage {
  type: "issues";
  projectId: string;
  rid?: string;
  error?: string;
}

/** What an empty description starts as when it is first edited on the issue page. */
export const DESCRIPTION_TEMPLATE = "## Goal\n\n## Scope\n\n## Done when\n- [ ] \n\n## Validation\n";

/**
 * The description without its empty sections: a `## heading` whose body is blank or only empty checklist items (`- [ ]`)
 * is dropped, and text before the first heading stays. The untouched template comes out as "".
 */
export function visibleDescription(description: string | undefined): string {
  const parts = (description ?? "").split(/^(?=## )/m);
  const body = (part: string) => part.replace(/^## .*$/m, "").replace(/^\s*[-*]\s*\[ \]\s*$/gm, "").trim();
  return parts.filter((part) => (part.startsWith("## ") ? body(part) : part.trim())).join("").trim();
}
/** Whether a description says anything beyond the template's headings and empty checklist items. */
export const hasContent = (description: string | undefined): boolean => visibleDescription(description) !== "";

export const isType = (v: unknown): v is IssueType => typeof v === "string" && (TYPES as string[]).includes(v);
export const isStatus = (v: unknown): v is Status => typeof v === "string" && (STATUSES as string[]).includes(v);

// ---------- keys ----------

/** Up to three letters of the project's name, uppercase, or ISS when it has none. "my-app" is MYA. */
export function prefixFor(name: string): string {
  return (name.match(/[A-Za-z]/g) ?? []).slice(0, 3).join("").toUpperCase() || "ISS";
}
export const keyNumber = (key: string): number => Number(key.slice(key.lastIndexOf("-") + 1));
export const KEY_PATTERN = /^[A-Z]{1,3}-\d{1,9}$/;
/** What a person or an agent typed, as a key: trimmed and uppercased. */
export const normalizeKey = (value: unknown): string => (typeof value === "string" ? value.trim().toUpperCase() : "");

export const emptyStore = (projectName: string): Store => ({ v: FORMAT_VERSION, prefix: prefixFor(projectName), next: 1, issues: [] });

// ---------- storage ----------

const key = (projectId: string) => `issues:${projectId}`;
const legacyKey = (projectId: string) => `tasks:${projectId}`;

const isShape = (v: unknown, version: number): boolean => !!v && typeof v === "object" && (v as { v?: unknown }).v === version && Array.isArray((v as Store).issues) && typeof (v as Store).prefix === "string" && typeof (v as Store).next === "number";
const isStore = (v: unknown): v is Store => isShape(v, FORMAT_VERSION);

/** Format 2, before types and relations: every issue is a task. */
export function migrateV2(value: unknown): Store | null {
  if (!isShape(value, 2)) return null;
  const old = value as Omit<Store, "v">;
  return { v: FORMAT_VERSION, prefix: old.prefix, next: old.next, issues: old.issues.map((i) => ({ ...i, type: "task" as const })) };
}

/** The old Tasks format, one project's list: {id, title, notes?, status todo|doing|done, links?.branch, createdAt, updatedAt}. */
export interface LegacyTask {
  id?: unknown;
  title?: unknown;
  notes?: unknown;
  status?: unknown;
  links?: { branch?: unknown };
  createdAt?: unknown;
  updatedAt?: unknown;
}

/**
 * Tasks as a store: oldest first get the numbers 1, 2, 3, notes become the description, doing becomes in_progress, and each
 * issue starts a log with "Imported from Tasks" by you at the time the task was made. Entries that aren't tasks are dropped.
 */
export function migrateTasks(value: unknown, projectName: string): Store {
  const store = emptyStore(projectName);
  if (!Array.isArray(value)) return store;
  const old = (value as LegacyTask[])
    .filter((t) => t && typeof t.title === "string" && typeof t.status === "string")
    .map((t, i) => ({ t, i, at: typeof t.createdAt === "number" ? t.createdAt : 0 }))
    .sort((a, b) => a.at - b.at || a.i - b.i);
  for (const { t, at } of old) {
    const status: Status = t.status === "doing" ? "in_progress" : t.status === "done" ? "done" : "todo";
    const issue: Issue = {
      key: `${store.prefix}-${store.next++}`,
      title: (t.title as string).slice(0, LIMITS.title),
      status,
      type: "task",
      sessions: [],
      log: [{ at, by: "you", kind: "created", text: "Imported from Tasks" }],
      createdAt: at,
      updatedAt: typeof t.updatedAt === "number" ? t.updatedAt : at,
    };
    if (typeof t.notes === "string" && t.notes.trim()) issue.description = t.notes.slice(0, LIMITS.description);
    if (typeof t.links?.branch === "string" && t.links.branch) issue.links = { branch: t.links.branch.slice(0, LIMITS.branch) };
    store.issues.push(issue);
  }
  return store;
}

/** A project's store. Pages and main both read through here: a project that still has only old tasks reads as migrated. */
export async function readStore(projectId: string, projectName: string): Promise<Store> {
  const value = await wings.storage.get(key(projectId));
  if (isStore(value)) return value;
  const v2 = migrateV2(value);
  if (v2) return v2;
  return migrateTasks(await wings.storage.get(legacyKey(projectId)), projectName);
}

/** The size of a store as saved: UTF-8 bytes of its JSON. */
export const estimateBytes = (store: Store): number => new TextEncoder().encode(JSON.stringify(store)).length;

export async function writeStore(projectId: string, store: Store): Promise<void> {
  const bytes = estimateBytes(store);
  if (bytes > BUDGET) throw new Error(`This project's issues take ${Math.round(bytes / 1000)} KB, over the ${Math.round(BUDGET / 1000)} KB a project may use. Remove finished issues first.`);
  await wings.storage.set(key(projectId), store);
}

/** Once per project: saves a format 2 store as format 3, or old tasks as issues (then drops the old entry). A no-op when the store is current. */
export async function migrateLegacy(projectId: string, projectName: string): Promise<boolean> {
  const current = await wings.storage.get(key(projectId));
  if (isStore(current)) return false;
  const v2 = migrateV2(current);
  if (v2) {
    await writeStore(projectId, v2);
    return true;
  }
  const old = await wings.storage.get(legacyKey(projectId));
  if (!Array.isArray(old)) return false;
  await writeStore(projectId, migrateTasks(old, projectName));
  await wings.storage.delete(legacyKey(projectId));
  return true;
}

// ---------- who wrote it ----------

/** What the pane list says about the calling terminal: its project, and the Claude session as an Actor. */
export interface PaneLike {
  paneId: string;
  project: string;
  projectId: string;
  session: { sessionId: string; name: string | null } | null;
}
export function resolvePane(panes: PaneLike[], paneId: string | null): { projectId: string; projectName: string; actor: Actor } {
  const pane = panes.find((p) => p.paneId === paneId);
  if (!pane) throw new Error("Can't tell which project this call comes from. Call it from a terminal in Wings.");
  // A tool call comes from Claude, so a pane with no session is a race at startup: still an agent, just without an id.
  const actor: Actor = pane.session ? { id: pane.session.sessionId, ...(pane.session.name ? { name: pane.session.name } : {}) } : { id: "" };
  return { projectId: pane.projectId, projectName: pane.project, actor };
}

export const actorName = (by: Actor): string => (by === "you" ? "You" : by === "git" ? "Git" : by.name || "Claude");
/** The Claude session of an entry's author, or null for you and git. */
export const isClaude = (by: Actor): by is { id: string; name?: string } => by !== "you" && by !== "git";

// ---------- operations ----------

function text(value: unknown, field: string, max: number): string {
  if (typeof value !== "string") throw new Error(`${field} must be text.`);
  if (value.length > max) throw new Error(`${field} is limited to ${max.toLocaleString("en")} characters.`);
  return value.trim();
}

/** Records that `actor`'s session worked on `issue`: adds it, or refreshes its name and time. Users aren't sessions. */
function touch(issue: Issue, actor: Actor, now: number): void {
  issue.updatedAt = now;
  if (!isClaude(actor) || !actor.id) return;
  const known = issue.sessions.some((s) => s.id === actor.id);
  issue.sessions = known
    ? issue.sessions.map((s) => (s.id === actor.id ? { ...s, seen: now, ...(actor.name ? { name: actor.name } : {}) } : s))
    : [...issue.sessions, { id: actor.id, ...(actor.name ? { name: actor.name } : {}), seen: now }];
}

function append(issue: Issue, entries: LogEntry[]): void {
  if (issue.log.length + entries.length > LIMITS.log) throw new Error(`${issue.key} already has ${LIMITS.log} log entries, the most one issue keeps.`);
  issue.log = [...issue.log, ...entries];
}

const SHA = /^[0-9a-f]{7,40}$/;

function checkPr(value: string): string {
  const pr = text(value, "pr", LIMITS.pr);
  if (pr && !/^https:\/\/[^\s]+$/.test(pr)) throw new Error("pr must be an https URL.");
  return pr;
}

/** Applies `link` changes of an update to `next`, and returns the log entries they make. */
function applyLinks(next: Issue, op: Extract<Op, { kind: "update" }>, by: Actor, now: number): LogEntry[] {
  const entries: LogEntry[] = [];
  const line = (t: string) => entries.push({ at: now, by, kind: "link", text: t });
  const links = { ...next.links };
  if (op.branch !== undefined) {
    const branch = text(op.branch, "branch", LIMITS.branch);
    if (/\s/.test(branch)) throw new Error("branch can't contain spaces.");
    if (branch !== (links.branch ?? "")) {
      if (branch) links.branch = branch;
      else delete links.branch;
      line(branch ? `Branch: ${branch}` : "Branch removed");
    }
  }
  if (op.pr !== undefined) {
    const pr = checkPr(op.pr);
    if (pr !== (links.pr ?? "")) {
      if (pr) links.pr = pr;
      else delete links.pr;
      line(pr ? `PR: ${pr}` : "PR removed");
    }
  }
  if (op.commit !== undefined) {
    const commit = text(op.commit, "commit", 40).toLowerCase();
    if (!SHA.test(commit)) throw new Error("commit must be a git commit hash of 7 to 40 hex digits.");
    const short = commit.slice(0, 7);
    if (!(links.commits ?? []).some((c) => c.startsWith(short) || short.startsWith(c))) {
      links.commits = [...(links.commits ?? []), short];
      line(`Commit: ${short}`);
    }
  }
  if (Object.keys(links).length) next.links = links;
  else delete next.links;
  return entries;
}

export interface Applied {
  store: Store;
  issue: Issue | null;
  /** False when the op asked for what was already so. Nothing is saved then. */
  changed: boolean;
}

/** Like `append`, but drops what doesn't fit instead of throwing: for entries the plugin writes on its own. */
function appendLoose(issue: Issue, entries: LogEntry[]): void {
  issue.log = [...issue.log, ...entries.slice(0, Math.max(0, LIMITS.log - issue.log.length))];
}

function keyList(value: unknown, field: string): string[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.some((v) => typeof v !== "string")) throw new Error(`${field} must be a list of issue keys.`);
  return [...new Set((value as string[]).map(normalizeKey))].filter(Boolean);
}

/** The issues whose parent is `key`. */
export const childrenOf = (issues: Issue[], key: string): Issue[] => issues.filter((i) => i.parent === key);
/** The issues `key` is blocking: the ones that list it as blocking them. */
export const blocksOf = (issues: Issue[], key: string): Issue[] => issues.filter((i) => i.blockedBy?.includes(key));
/** Done out of all, not counting canceled ones: "3 of 7 done". */
export function progress(children: Issue[]): { done: number; total: number } {
  const live = children.filter((c) => c.status !== "canceled");
  return { done: live.filter((c) => c.status === "done").length, total: live.length };
}
/** The epics and features `issue` may sit under: not itself, and not one of its own children, grandchildren and so on. */
export function parentChoices(issues: Issue[], issue: Issue): Issue[] {
  const below = new Set<string>([issue.key]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const i of issues) if (i.parent && below.has(i.parent) && !below.has(i.key)) (below.add(i.key), (grew = true));
  }
  return issues.filter((i) => canParent(i.type) && !below.has(i.key)).sort((a, b) => keyNumber(a.key) - keyNumber(b.key));
}

/**
 * Applies the type and link changes of an op to `self`, a copy in `map`, and to the other issues it touches, which are
 * copies in `map` too. Every change writes an entry on both issues. Returns whether anything changed.
 */
function relate(map: Map<string, Issue>, self: Issue, op: Relations, by: Actor, now: number): boolean {
  let changed = false;
  const log = (issue: Issue | undefined, kind: LogKind, text: string) => {
    if (!issue) return;
    append(issue, [{ at: now, by, kind, text }]);
    if (issue !== self) issue.updatedAt = now;
  };
  const other = (raw: string): Issue => {
    if (!KEY_PATTERN.test(raw)) throw new Error(`${raw} isn't an issue key, like WIN-12.`);
    if (raw === self.key) throw new Error(`${self.key} can't link to itself.`);
    const found = map.get(raw);
    if (!found) throw new Error(`No issue ${raw} in this project. Use list to see the keys.`);
    return found;
  };
  const without = (list: string[] | undefined, key: string) => {
    const rest = (list ?? []).filter((k) => k !== key);
    return rest.length ? rest : undefined;
  };
  const set = (issue: Issue, field: "related" | "blockedBy", list: string[] | undefined) => {
    if (list) issue[field] = list;
    else delete issue[field];
  };

  if (op.type !== undefined) {
    if (!isType(op.type)) throw new Error(`type must be one of ${TYPES.join(", ")}.`);
    if (op.type !== self.type) {
      if (!canParent(op.type) && [...map.values()].some((i) => i.parent === self.key)) throw new Error(`${self.key} has child issues, so it has to stay an epic or a feature. Move them to another parent first.`);
      log(self, "type", `Type: ${TYPE_LABEL[self.type]} → ${TYPE_LABEL[op.type]}`);
      self.type = op.type;
      changed = true;
    }
  }

  if (op.parent !== undefined) {
    if (typeof op.parent !== "string") throw new Error("parent must be an issue key, or an empty string to clear it.");
    const wanted = normalizeKey(op.parent);
    if (!wanted) {
      if (self.parent) {
        log(self, "link", `Parent removed (was ${self.parent})`);
        log(map.get(self.parent), "link", `Child removed: ${self.key}`);
        delete self.parent;
        changed = true;
      }
    } else if (wanted !== self.parent) {
      const parent = other(wanted);
      if (!canParent(parent.type)) throw new Error(`${parent.key} is a ${TYPE_LABEL[parent.type].toLowerCase()}. Only an epic or a feature can be a parent: change its type first.`);
      const seen = new Set<string>();
      for (let up: Issue | undefined = parent; up && !seen.has(up.key); up = up.parent ? map.get(up.parent) : undefined) {
        if (up.key === self.key) throw new Error(`${parent.key} is under ${self.key} already, so ${self.key} can't go under it.`);
        seen.add(up.key);
      }
      if (self.parent) log(map.get(self.parent), "link", `Child removed: ${self.key}`);
      log(self, "link", `Parent: ${parent.key}`);
      log(parent, "link", `Child: ${self.key}`);
      self.parent = parent.key;
      changed = true;
    }
  }

  for (const raw of keyList(op.relatedAdd, "related_add")) {
    const peer = other(raw);
    if (self.related?.includes(raw)) continue;
    if ((self.related?.length ?? 0) >= MAX_LINKS) throw new Error(`${self.key} already has ${MAX_LINKS} related issues, the most one keeps.`);
    if ((peer.related?.length ?? 0) >= MAX_LINKS) throw new Error(`${peer.key} already has ${MAX_LINKS} related issues, the most one keeps.`);
    self.related = [...(self.related ?? []), raw];
    peer.related = [...(peer.related ?? []), self.key];
    log(self, "link", `Linked to ${raw} as related`);
    log(peer, "link", `Linked to ${self.key} as related`);
    changed = true;
  }
  for (const raw of keyList(op.relatedRemove, "related_remove")) {
    if (!self.related?.includes(raw)) continue;
    set(self, "related", without(self.related, raw));
    const peer = map.get(raw);
    if (peer) set(peer, "related", without(peer.related, self.key));
    log(self, "link", `Unlinked from ${raw} (related)`);
    log(peer, "link", `Unlinked from ${self.key} (related)`);
    changed = true;
  }

  for (const raw of keyList(op.blockedByAdd, "blocked_by_add")) {
    const blocker = other(raw);
    if (self.blockedBy?.includes(raw)) continue;
    if ((self.blockedBy?.length ?? 0) >= MAX_LINKS) throw new Error(`${self.key} is already blocked by ${MAX_LINKS} issues, the most one keeps.`);
    // A blocker that waits on this issue, however far down the chain, would leave both stuck.
    const seen = new Set<string>();
    const stack = [blocker];
    for (let next = stack.pop(); next; next = stack.pop()) {
      if (seen.has(next.key)) continue;
      seen.add(next.key);
      if (next.key === self.key) throw new Error(`${raw} is waiting on ${self.key} already, so ${self.key} can't wait on it: they would block each other.`);
      for (const k of next.blockedBy ?? []) if (map.has(k)) stack.push(map.get(k)!);
    }
    self.blockedBy = [...(self.blockedBy ?? []), raw];
    log(self, "link", `Blocked by ${raw}`);
    log(blocker, "link", `Blocks ${self.key}`);
    changed = true;
  }
  for (const raw of keyList(op.blockedByRemove, "blocked_by_remove")) {
    if (!self.blockedBy?.includes(raw)) continue;
    set(self, "blockedBy", without(self.blockedBy, raw));
    log(self, "link", `No longer blocked by ${raw}`);
    log(map.get(raw), "link", `No longer blocks ${self.key}`);
    changed = true;
  }
  return changed;
}

/** Whether an op carries a link or type change, so the cheap path can skip copying every issue. */
const hasRelations = (op: Relations): boolean => [op.type, op.parent, op.relatedAdd, op.relatedRemove, op.blockedByAdd, op.blockedByRemove].some((v) => v !== undefined);

/** `store` with `op` applied by `by`, as a new store. Throws an Error that says what was wrong. */
export function applyOp(store: Store, op: Op, by: Actor = "you", now = Date.now()): Applied {
  if (op.kind === "create") {
    const title = text(op.title, "title", LIMITS.title);
    if (!title) throw new Error("title is required.");
    const description = op.description === undefined ? "" : text(op.description, "description", LIMITS.description);
    if (op.status !== undefined && !isStatus(op.status)) throw new Error(`status must be one of ${STATUSES.join(", ")}.`);
    if (op.type !== undefined && !isType(op.type)) throw new Error(`type must be one of ${TYPES.join(", ")}.`);
    if (store.issues.length >= LIMITS.issues) throw new Error(`A project holds up to ${LIMITS.issues} issues. Remove one first.`);
    const issue: Issue = { key: `${store.prefix}-${store.next}`, title, status: op.status ?? "todo", type: op.type ?? "task", sessions: [], log: [{ at: now, by, kind: "created", text: "Created" }], createdAt: now, updatedAt: now };
    if (hasContent(description)) issue.description = description;
    touch(issue, by, now);
    if (op.branch !== undefined) append(issue, applyLinks(issue, { kind: "update", key: issue.key, branch: op.branch }, by, now));
    let issues = store.issues;
    if (hasRelations({ ...op, type: undefined })) {
      const map = new Map(store.issues.map((i) => [i.key, { ...i }]));
      map.set(issue.key, issue);
      relate(map, issue, { ...op, type: undefined }, by, now);
      issues = store.issues.map((i) => map.get(i.key)!);
    }
    return { store: { ...store, next: store.next + 1, issues: [...issues, issue] }, issue, changed: true };
  }

  const wanted = normalizeKey(op.key);
  const index = store.issues.findIndex((i) => i.key === wanted);
  if (index < 0) throw new Error(`No issue ${String(op.key)} in this project. Use list to see the keys.`);
  const current = store.issues[index];
  if (op.kind === "remove") {
    // Whatever pointed at it lets go, and says why in its log. The counter stays where it is, so the key is never handed out again.
    const issues = store.issues
      .filter((_, i) => i !== index)
      .map((i) => {
        const notes: string[] = [];
        let next = i;
        const edit = () => (next === i ? (next = { ...i }) : next);
        if (i.parent === wanted) (delete edit().parent, notes.push(`Parent ${wanted} was deleted`));
        for (const [field, note] of [["related", "unlinked (related)"], ["blockedBy", "no longer blocking"]] as const) {
          if (!i[field]?.includes(wanted)) continue;
          const rest = i[field].filter((k) => k !== wanted);
          if (rest.length) edit()[field] = rest;
          else delete edit()[field];
          notes.push(`${wanted} was deleted: ${note}`);
        }
        if (next === i) return i;
        appendLoose(next, notes.map((t) => ({ at: now, by, kind: "link" as const, text: t })));
        next.updatedAt = now;
        return next;
      });
    return { store: { ...store, issues }, issue: current, changed: true };
  }

  const next: Issue = { ...current };
  let entries: LogEntry[] = [];
  let changed = false;
  let others: Issue[] | null = null;
  if (op.kind === "note") {
    const note = text(op.text, "text", LIMITS.note);
    if (!note) throw new Error("text is required.");
    entries = [{ at: now, by, kind: "note", text: note }];
    changed = true;
  } else {
    if (op.title !== undefined) {
      const title = text(op.title, "title", LIMITS.title);
      if (!title) throw new Error("title can't be empty.");
      if (title !== next.title) ((next.title = title), (changed = true));
    }
    if (op.description !== undefined) {
      const typed = text(op.description, "description", LIMITS.description);
      const description = hasContent(typed) ? typed : "";
      if (description !== (next.description ?? "")) {
        if (description) next.description = description;
        else delete next.description;
        changed = true;
      }
    }
    if (op.status !== undefined) {
      if (!isStatus(op.status)) throw new Error(`status must be one of ${STATUSES.join(", ")}.`);
      if (op.status !== next.status) {
        entries.push({ at: now, by, kind: "status", text: `${STATUS_LABEL[next.status]} → ${STATUS_LABEL[op.status]}` });
        next.status = op.status;
        changed = true;
      }
    }
    const linkEntries = applyLinks(next, op, by, now);
    entries.push(...linkEntries);
    if (linkEntries.length) changed = true;
    if (hasRelations(op)) {
      // The other issues are copies too, so a failure half way leaves the store as it was.
      const map = new Map(store.issues.map((i) => [i.key, { ...i }]));
      map.set(next.key, next);
      if (relate(map, next, op, by, now)) {
        changed = true;
        others = store.issues.map((i) => map.get(i.key)!);
      }
    }
  }
  if (!changed) return { store, issue: current, changed: false };
  append(next, entries);
  touch(next, by, now);
  const issues = others ?? store.issues.slice();
  if (!others) issues[index] = next;
  return { store: { ...store, issues }, issue: next, changed: true };
}

// ---------- reading ----------

/** The open issues of the short list and the project section: in progress first, then in review, then to do, each oldest key first. */
export function openIssues(issues: Issue[]): Issue[] {
  const order: Record<Status, number> = { in_progress: 0, in_review: 1, todo: 2, done: 3, canceled: 3 };
  return issues.filter((i) => i.status !== "done" && i.status !== "canceled").sort((a, b) => order[a.status] - order[b.status] || keyNumber(a.key) - keyNumber(b.key));
}

/** The board's columns: to do by key, the rest by latest activity. Canceled only when asked for. */
export function boardColumns(issues: Issue[], showCanceled = false): { status: Status; issues: Issue[] }[] {
  return (showCanceled ? STATUSES : FLOW).map((status) => ({
    status,
    issues: issues.filter((i) => i.status === status).sort((a, b) => (status === "todo" ? keyNumber(a.key) - keyNumber(b.key) : b.updatedAt - a.updatedAt)),
  }));
}

export const findIssue = (store: Store | null, issueKey: string): Issue | null => store?.issues.find((i) => i.key === normalizeKey(issueKey)) ?? null;

/** The session that touched the issue last, or null. Resume picks this one. */
export const lastSession = (issue: Issue): SessionRef | null => issue.sessions.reduce<SessionRef | null>((a, s) => (!a || s.seen > a.seen ? s : a), null);

// ---------- starting Claude in a pane ----------

/**
 * Pane commands are typed into the user's shell: Wings takes command words of letters, digits and -_./:=@%+, and
 * quotes each `args` value as one word itself, spaces and all. Nothing here is user text: a key and a session id are
 * checked first.
 */
export const PLAIN_WORD = /^[A-Za-z0-9_.\/:=@%+,-]+$/;
export function resumeCommand(sessionId: string): string {
  if (!/^[0-9A-Za-z-]{8,64}$/.test(sessionId)) throw new Error("That isn't a Claude session id.");
  return `claude --resume ${sessionId}`;
}
/** `claude` with its first prompt, as `openPane`'s `command` and `args`. */
export function agentCommand(issueKey: string): { command: string; args: string[] } {
  if (!KEY_PATTERN.test(issueKey)) throw new Error("That isn't an issue key.");
  const prompt = `Continue issue ${issueKey}. Read it with tasks__get, do the work, and log progress with tasks__log and the status with tasks__update as you go.`;
  return { command: "claude", args: [prompt] };
}
