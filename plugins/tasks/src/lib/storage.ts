// Issues live in wings.storage, one entry per project id (`issues:<projectId>`). Only the main script writes them: pages ask it to
// apply operations and the tools run the same ones, so a page and an agent never overwrite each other. This is the only module that
// reads and writes the issues.

import { BUDGET, FORMAT_VERSION, LIMITS, prefixFor, type Issue, type Status, type Store } from "./domain";

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
