// The operations a page or a tool asks for, applied to a store: `applyOp` gives the changed copy, or throws an Error that says
// what was wrong. Nothing is saved here.

import { checkEnum, hasContent, LIMITS, normalizeKey, PRIORITIES, PRIORITY_LABEL, STATUS_LABEL, STATUSES, TYPES, type Actor, type Issue, type LogEntry, type Op, type Relations, type Store } from "./domain";
import { upsertFiles } from "./files";
import { append, appendLoose, touch } from "./log";
import { descendantsOf, relate } from "./relations";

function text(value: unknown, field: string, max: number): string {
  if (typeof value !== "string") throw new Error(`${field} must be text.`);
  if (value.length > max) throw new Error(`${field} is limited to ${max.toLocaleString("en")} characters.`);
  return value.trim();
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
  /** Sets the branch or the PR to `value`, or clears it for "", and logs it as `label` when that changes it. */
  const setLink = (field: "branch" | "pr", value: string, label: string) => {
    if (value === (links[field] ?? "")) return;
    if (value) links[field] = value;
    else delete links[field];
    line(value ? `${label}: ${value}` : `${label} removed`);
  };
  if (op.branch !== undefined) {
    const branch = text(op.branch, "branch", LIMITS.branch);
    if (/\s/.test(branch)) throw new Error("branch can't contain spaces.");
    setLink("branch", branch, "Branch");
  }
  if (op.pr !== undefined) setLink("pr", checkPr(op.pr), "PR");
  if (op.commit !== undefined) {
    const commit = text(op.commit, "commit", LIMITS.commit).toLowerCase();
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
  /** The issue the op made or changed: for a removal, the one that is gone. */
  issue: Issue;
  /** False when the op asked for what was already so. Nothing is saved then. */
  changed: boolean;
}

/** Whether an op carries a link or type change, so the cheap path can skip copying every issue. */
const hasRelations = (op: Relations): boolean => [op.type, op.parent, op.relatedAdd, op.relatedRemove, op.blockedByAdd, op.blockedByRemove].some((v) => v !== undefined);

/**
 * The store's issues with the type and link changes of `op` made to `issue` and to the issues they touch, or null when
 * it changes none. The other issues are copies too, so a failure half way leaves the store as it was.
 */
function withRelations(store: Store, issue: Issue, op: Relations, by: Actor, now: number): Issue[] | null {
  if (!hasRelations(op)) return null;
  const map = new Map(store.issues.map((i) => [i.key, { ...i }]));
  map.set(issue.key, issue);
  return relate(map, issue, op, by, now) ? store.issues.map((i) => map.get(i.key)!) : null;
}

/** `store` with `op` applied by `by`, as a new store. Throws an Error that says what was wrong. */
export function applyOp(store: Store, op: Op, by: Actor = "you", now = Date.now()): Applied {
  if (op.kind === "create") return createIssue(store, op, by, now);
  const wanted = normalizeKey(op.key);
  const index = store.issues.findIndex((i) => i.key === wanted);
  if (index < 0) throw new Error(`No issue ${wanted} in this project. Use list to see the keys.`);
  if (op.kind === "remove") return removeIssue(store, index, op, by, now);
  return updateIssue(store, index, op, by, now);
}

function createIssue(store: Store, op: Extract<Op, { kind: "create" }>, by: Actor, now: number): Applied {
  const title = text(op.title, "title", LIMITS.title);
  if (!title) throw new Error("title is required.");
  const description = op.description === undefined ? "" : text(op.description, "description", LIMITS.description);
  if (op.status !== undefined) checkEnum(op.status, STATUSES, "status");
  if (op.type !== undefined) checkEnum(op.type, TYPES, "type");
  if (op.priority !== undefined) checkEnum(op.priority, PRIORITIES, "priority");
  if (store.issues.length >= LIMITS.issues) throw new Error(`A project holds up to ${LIMITS.issues} issues. Remove one first.`);
  const issue: Issue = { key: `${store.prefix}-${store.next}`, title, status: op.status ?? "todo", type: op.type ?? "task", sessions: [], log: [{ at: now, by, kind: "created", text: "Created" }], createdAt: now, updatedAt: now };
  if (hasContent(description)) issue.description = description;
  if (op.priority && op.priority !== "none") issue.priority = op.priority;
  touch(issue, by, now);
  if (op.branch !== undefined) append(issue, applyLinks(issue, { kind: "update", key: issue.key, branch: op.branch }, by, now));
  // Its type is set already: only its links go through `relate`.
  const issues = withRelations(store, issue, { ...op, type: undefined }, by, now) ?? store.issues;
  return { store: { ...store, next: store.next + 1, issues: [...issues, issue] }, issue, changed: true };
}

function removeIssue(store: Store, index: number, op: Extract<Op, { kind: "remove" }>, by: Actor, now: number): Applied {
  const current = store.issues[index];
  // Whatever pointed at it lets go, and says why in its log. The counter stays where it is, so the key is never handed out again.
  const gone = new Set([current.key, ...(op.children === true ? descendantsOf(store.issues, current.key).map((i) => i.key) : [])]);
  const issues = store.issues
    .filter((i) => !gone.has(i.key))
    .map((i) => {
      const notes: string[] = [];
      let next = i;
      const edit = () => (next === i ? (next = { ...i }) : next);
      if (i.parent && gone.has(i.parent)) (notes.push(`Parent ${i.parent} was deleted`), delete edit().parent);
      for (const [field, note] of [["related", "unlinked (related)"], ["blockedBy", "no longer blocking"]] as const) {
        const hit = i[field]?.filter((k) => gone.has(k));
        if (!hit?.length) continue;
        const rest = i[field]!.filter((k) => !gone.has(k));
        if (rest.length) edit()[field] = rest;
        else delete edit()[field];
        for (const k of hit) notes.push(`${k} was deleted: ${note}`);
      }
      if (next === i) return i;
      appendLoose(next, notes.map((t) => ({ at: now, by, kind: "link" as const, text: t })));
      next.updatedAt = now;
      return next;
    });
  return { store: { ...store, issues }, issue: current, changed: true };
}

/** `store` with a note, the files a journal reported, or an update's changes made to the issue at `index`. */
function updateIssue(store: Store, index: number, op: Extract<Op, { kind: "files" | "note" | "update" }>, by: Actor, now: number): Applied {
  const current = store.issues[index];
  const next: Issue = { ...current };
  let entries: LogEntry[] = [];
  let changed = false;
  let others: Issue[] | null = null;
  if (op.kind === "files") {
    // No log entry: the log is for people and decisions, and this can't fail on its limit.
    const files = upsertFiles(next.files, Array.isArray(op.files) ? op.files : [], by, now);
    if (JSON.stringify(files) !== JSON.stringify(next.files ?? [])) ((next.files = files), (changed = true));
  } else if (op.kind === "note") {
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
      checkEnum(op.status, STATUSES, "status");
      if (op.status !== next.status) {
        entries.push({ at: now, by, kind: "status", text: `${STATUS_LABEL[next.status]} → ${STATUS_LABEL[op.status]}` });
        next.status = op.status;
        changed = true;
      }
    }
    if (op.priority !== undefined) {
      checkEnum(op.priority, PRIORITIES, "priority");
      if (op.priority !== (next.priority ?? "none")) {
        entries.push({ at: now, by, kind: "priority", text: `Priority: ${PRIORITY_LABEL[next.priority ?? "none"]} → ${PRIORITY_LABEL[op.priority]}` });
        if (op.priority === "none") delete next.priority;
        else next.priority = op.priority;
        changed = true;
      }
    }
    const linkEntries = applyLinks(next, op, by, now);
    entries.push(...linkEntries);
    if (linkEntries.length) changed = true;
    others = withRelations(store, next, op, by, now);
    if (others) changed = true;
  }
  if (!changed) return { store, issue: current, changed: false };
  append(next, entries);
  touch(next, by, now);
  const issues = others ?? store.issues.slice();
  if (!others) issues[index] = next;
  return { store: { ...store, issues }, issue: next, changed: true };
}
