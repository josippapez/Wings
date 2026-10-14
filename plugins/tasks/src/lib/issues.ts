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
export const FORMAT_VERSION = 2;

export type Status = "todo" | "in_progress" | "in_review" | "done" | "canceled";
export const STATUSES: Status[] = ["todo", "in_progress", "in_review", "done", "canceled"];
/** The columns of the board, in the order an issue moves through them. Canceled sits behind a toggle. */
export const FLOW: Status[] = ["todo", "in_progress", "in_review", "done"];
export const STATUS_LABEL: Record<Status, string> = { todo: "To do", in_progress: "In progress", in_review: "In review", done: "Done", canceled: "Canceled" };

/** Who wrote a log entry: the user in the UI, or the Claude session an agent's tool call came from. */
export type Actor = "you" | { id: string; name?: string };
export type LogKind = "created" | "status" | "note" | "link";
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
export type Op =
  | { kind: "create"; title: string; description?: string; status?: Status; branch?: string }
  | { kind: "update"; key: string; title?: string; description?: string; status?: Status; branch?: string; pr?: string; commit?: string }
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

const isStore = (v: unknown): v is Store => !!v && typeof v === "object" && (v as Store).v === FORMAT_VERSION && Array.isArray((v as Store).issues) && typeof (v as Store).prefix === "string" && typeof (v as Store).next === "number";

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
  return migrateTasks(await wings.storage.get(legacyKey(projectId)), projectName);
}

/** The size of a store as saved: UTF-8 bytes of its JSON. */
export const estimateBytes = (store: Store): number => new TextEncoder().encode(JSON.stringify(store)).length;

export async function writeStore(projectId: string, store: Store): Promise<void> {
  const bytes = estimateBytes(store);
  if (bytes > BUDGET) throw new Error(`This project's issues take ${Math.round(bytes / 1000)} KB, over the ${Math.round(BUDGET / 1000)} KB a project may use. Remove finished issues first.`);
  await wings.storage.set(key(projectId), store);
}

/** Once per project: saves old tasks as issues, then drops the old entry. A no-op when there is nothing old or the store exists. */
export async function migrateLegacy(projectId: string, projectName: string): Promise<boolean> {
  if (isStore(await wings.storage.get(key(projectId)))) return false;
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

export const actorName = (by: Actor): string => (by === "you" ? "You" : by.name || "Claude");

// ---------- operations ----------

function text(value: unknown, field: string, max: number): string {
  if (typeof value !== "string") throw new Error(`${field} must be text.`);
  if (value.length > max) throw new Error(`${field} is limited to ${max.toLocaleString("en")} characters.`);
  return value.trim();
}

/** Records that `actor`'s session worked on `issue`: adds it, or refreshes its name and time. Users aren't sessions. */
function touch(issue: Issue, actor: Actor, now: number): void {
  issue.updatedAt = now;
  if (actor === "you" || !actor.id) return;
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

/** `store` with `op` applied by `by`, as a new store. Throws an Error that says what was wrong. */
export function applyOp(store: Store, op: Op, by: Actor = "you", now = Date.now()): Applied {
  if (op.kind === "create") {
    const title = text(op.title, "title", LIMITS.title);
    if (!title) throw new Error("title is required.");
    const description = op.description === undefined ? "" : text(op.description, "description", LIMITS.description);
    if (op.status !== undefined && !isStatus(op.status)) throw new Error(`status must be one of ${STATUSES.join(", ")}.`);
    if (store.issues.length >= LIMITS.issues) throw new Error(`A project holds up to ${LIMITS.issues} issues. Remove one first.`);
    const issue: Issue = { key: `${store.prefix}-${store.next}`, title, status: op.status ?? "todo", sessions: [], log: [{ at: now, by, kind: "created", text: "Created" }], createdAt: now, updatedAt: now };
    if (description) issue.description = description;
    touch(issue, by, now);
    if (op.branch !== undefined) append(issue, applyLinks(issue, { kind: "update", key: issue.key, branch: op.branch }, by, now));
    return { store: { ...store, next: store.next + 1, issues: [...store.issues, issue] }, issue, changed: true };
  }

  const wanted = normalizeKey(op.key);
  const index = store.issues.findIndex((i) => i.key === wanted);
  if (index < 0) throw new Error(`No issue ${String(op.key)} in this project. Use list to see the keys.`);
  const current = store.issues[index];
  // The counter stays where it is, so a removed key is never handed out again.
  if (op.kind === "remove") return { store: { ...store, issues: store.issues.filter((_, i) => i !== index) }, issue: current, changed: true };

  const next: Issue = { ...current };
  let entries: LogEntry[] = [];
  let changed = false;
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
      const description = text(op.description, "description", LIMITS.description);
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
  }
  if (!changed) return { store, issue: current, changed: false };
  append(next, entries);
  touch(next, by, now);
  const issues = store.issues.slice();
  issues[index] = next;
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
