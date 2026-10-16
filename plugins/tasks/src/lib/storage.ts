// Issues live in wings.storage, one entry per issue (`issues/<slot>/<KEY>`) and one for the project's prefix and counter
// (`issues/<slot>`), where the slot is a short id of the project's path. Only the main script writes them: pages ask it to apply
// operations and the tools run the same ones, so a page and an agent never overwrite each other. This is the only module that
// reads and writes the issues.

import { FORMAT_VERSION, ISSUE_BUDGET, keyNumber, LIMITS, prefixFor, type Issue, type Status, type Store } from "./domain";

export const emptyStore = (projectName: string): Store => ({ v: FORMAT_VERSION, prefix: prefixFor(projectName), next: 1, issues: [] });

// ---------- storage ----------

/**
 * A project's slot: 64 bits of a hash of its path, as 16 hex digits. A key is at most 128 characters and a path can be longer,
 * so the path itself is only kept once, in the project's entry, where `readStore` checks it.
 */
export function slotOf(projectId: string): string {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let i = 0; i < projectId.length; i++) {
    const c = projectId.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 2654435761);
    h2 = Math.imul(h2 ^ c, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return (h2 >>> 0).toString(16).padStart(8, "0") + (h1 >>> 0).toString(16).padStart(8, "0");
}

/** The project's entry: its path, prefix and counter. */
interface Project {
  v: typeof FORMAT_VERSION;
  path: string;
  prefix: string;
  next: number;
}

const projectKey = (projectId: string) => `issues/${slotOf(projectId)}`;
const issueKey = (projectId: string, key: string) => `${projectKey(projectId)}/${key}`;
/** Where a project's issues were before one entry per issue: all of them in one entry. */
const oldKey = (projectId: string) => `issues:${projectId}`;
const legacyKey = (projectId: string) => `tasks:${projectId}`;

const isShape = (v: unknown, version: number): boolean => !!v && typeof v === "object" && (v as { v?: unknown }).v === version && Array.isArray((v as Store).issues) && typeof (v as Store).prefix === "string" && typeof (v as Store).next === "number";
const isStore = (v: unknown): v is Store => isShape(v, FORMAT_VERSION);
const isProject = (v: unknown): v is Project => !!v && typeof v === "object" && (v as Project).v === FORMAT_VERSION && typeof (v as Project).path === "string" && typeof (v as Project).prefix === "string" && typeof (v as Project).next === "number";
const projectOf = (projectId: string, store: Store): Project => ({ v: FORMAT_VERSION, path: projectId, prefix: store.prefix, next: store.next });

/** A store as the entries that hold it, project entry last. */
export function entriesOf(projectId: string, store: Store): [string, unknown][] {
  return [...store.issues.map((i): [string, unknown] => [issueKey(projectId, i.key), i]), [projectKey(projectId), projectOf(projectId, store)]];
}

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

/** A project's store. Pages and main both read through here: a project that still has only older entries reads as migrated. */
export async function readStore(projectId: string, projectName: string): Promise<Store> {
  const entries = await wings.storage.entries(projectKey(projectId));
  const project = entries.find(([k]) => k === projectKey(projectId))?.[1];
  if (!isProject(project)) return readOld(projectId, projectName);
  if (project.path !== projectId) throw new Error(`Issues keeps ${project.path} where ${projectId} would go. Rename one of the two folders.`);
  const issues = entries.filter(([k]) => k.startsWith(`${projectKey(projectId)}/`)).map(([, v]) => v as Issue);
  return { v: FORMAT_VERSION, prefix: project.prefix, next: project.next, issues: issues.sort((a, b) => keyNumber(a.key) - keyNumber(b.key)) };
}

/** What a project held before one entry per issue: all of it in `issues:<path>` (format 3 or 2), or old tasks. */
async function readOld(projectId: string, projectName: string): Promise<Store> {
  const value = await wings.storage.get(oldKey(projectId));
  if (isStore(value)) return value;
  const v2 = migrateV2(value);
  if (v2) return v2;
  return migrateTasks(await wings.storage.get(legacyKey(projectId)), projectName);
}

/** The size of a value as saved: UTF-8 bytes of its JSON. */
export const estimateBytes = (value: unknown): number => new TextEncoder().encode(JSON.stringify(value)).length;

/**
 * Saves the change from `before` to `after`: the issues that differ, and the project's entry when its counter moved. The counter
 * goes first, so a crash in between can skip a number but never hand one out twice.
 */
export async function writeStore(projectId: string, before: Store, after: Store): Promise<void> {
  const old = new Map(before.issues.map((i) => [i.key, i]));
  const changed = after.issues.filter((i) => {
    const was = old.get(i.key);
    return was !== i && JSON.stringify(was) !== JSON.stringify(i);
  });
  for (const issue of changed) {
    const bytes = estimateBytes(issue);
    if (bytes > ISSUE_BUDGET) throw new Error(`${issue.key} would take ${Math.round(bytes / 1000)} KB, over the ${Math.round(ISSUE_BUDGET / 1000)} KB one issue may use. Carry on in a new issue.`);
  }
  if (after.next !== before.next || after.prefix !== before.prefix) await wings.storage.set(projectKey(projectId), projectOf(projectId, after));
  for (const issue of changed) await wings.storage.set(issueKey(projectId, issue.key), issue);
  const kept = new Set(after.issues.map((i) => i.key));
  for (const issue of before.issues) if (!kept.has(issue.key)) await wings.storage.delete(issueKey(projectId, issue.key));
}

/**
 * Once per project: saves what it held in older entries (format 3 or 2 in `issues:<path>`, or old tasks) as one entry per issue,
 * then drops the older entries. The project's entry is written last, so until then pages still read the older ones, and a
 * migration cut short starts again. A no-op when the project already has its entry.
 */
export async function migrateLegacy(projectId: string, projectName: string): Promise<boolean> {
  if (isProject(await wings.storage.get(projectKey(projectId)))) return false;
  const current = await wings.storage.get(oldKey(projectId));
  let store = isStore(current) ? current : migrateV2(current);
  if (!store) {
    const old = await wings.storage.get(legacyKey(projectId));
    if (!Array.isArray(old)) return false;
    store = migrateTasks(old, projectName);
  }
  for (const [key, value] of entriesOf(projectId, store)) await wings.storage.set(key, value);
  await wings.storage.delete(oldKey(projectId));
  await wings.storage.delete(legacyKey(projectId));
  return true;
}
