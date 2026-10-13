// Tasks live in wings.storage, one key per project id. Only the main script writes them: pages send it small
// operations (add, update, remove) and tools call the same ones, so a page and an agent never overwrite each other.
// Operations change one task, so there is no whole-list save to merge.

/**
 * Why these numbers: all of the plugin's storage is 1 MB. A task with a 160-character title and 1,000 characters of
 * notes is about 1.3 KB of JSON, so a full project of 100 such tasks is about 130 KB and a few of those still fit.
 * Real tasks are a title and a line, about 150 bytes, which makes a project about 15 KB and leaves room for dozens.
 * A broadcast is limited to 64,000 characters, and none carries task text from main to a page: it carries ids.
 */
export const LIMITS = { tasks: 100, title: 160, notes: 1000, branch: 200 } as const;

export type Status = "todo" | "doing" | "done";
export const STATUSES: Status[] = ["todo", "doing", "done"];
export const STATUS_LABEL: Record<Status, string> = { todo: "To do", doing: "Doing", done: "Done" };

export interface Task {
  id: string;
  title: string;
  notes?: string;
  status: Status;
  links?: { branch?: string };
  createdAt: number;
  updatedAt: number;
}

/** What a page or a tool asks main to do. `branch` "" clears it. */
export type Op =
  | { kind: "add"; title: string; notes?: string; branch?: string; status?: Status }
  | { kind: "update"; id: string; title?: string; notes?: string; branch?: string; status?: Status }
  | { kind: "remove"; id: string };

/** A page asks main to apply `op`; `rid` names the request. */
export interface OpMessage {
  type: "op";
  projectId: string;
  rid: string;
  op: Op;
}

/** Main tells every page that a project's tasks changed, so they read them again. `error` answers a failed `rid`. */
export interface TasksMessage {
  type: "tasks";
  projectId: string;
  rid?: string;
  error?: string;
}

const key = (id: string) => `tasks:${id}`;

const isStatus = (v: unknown): v is Status => v === "todo" || v === "doing" || v === "done";

/** Storage is ours, but keep a hand-edited or older value from breaking a page. */
function clean(value: unknown): Task[] {
  if (!Array.isArray(value)) return [];
  const tasks: Task[] = [];
  for (const t of value as Partial<Task>[]) {
    if (!t || typeof t.id !== "string" || typeof t.title !== "string" || !isStatus(t.status)) continue;
    const task: Task = {
      id: t.id,
      title: t.title.slice(0, LIMITS.title),
      status: t.status,
      createdAt: typeof t.createdAt === "number" ? t.createdAt : 0,
      updatedAt: typeof t.updatedAt === "number" ? t.updatedAt : 0,
    };
    if (typeof t.notes === "string" && t.notes) task.notes = t.notes.slice(0, LIMITS.notes);
    if (typeof t.links?.branch === "string" && t.links.branch) task.links = { branch: t.links.branch.slice(0, LIMITS.branch) };
    tasks.push(task);
  }
  return tasks;
}

export async function readTasks(projectId: string): Promise<Task[]> {
  return clean(await wings.storage.get(key(projectId)));
}

export async function writeTasks(projectId: string, tasks: Task[]): Promise<void> {
  await wings.storage.set(key(projectId), tasks);
}

function text(value: unknown, field: string, max: number): string {
  if (typeof value !== "string") throw new Error(`${field} must be text.`);
  if (value.length > max) throw new Error(`${field} is limited to ${max.toLocaleString("en")} characters.`);
  return value.trim();
}

function newId(taken: Set<string>): string {
  for (;;) {
    const id = Math.random().toString(36).slice(2, 10);
    if (id.length === 8 && !taken.has(id)) return id;
  }
}

/** `tasks` with `op` applied, as a new list. Throws an Error that says what was wrong. */
export function applyOp(tasks: Task[], op: Op, now = Date.now()): { tasks: Task[]; task: Task | null } {
  if (op.kind === "add") {
    const title = text(op.title, "title", LIMITS.title);
    if (!title) throw new Error("title is required.");
    const notes = op.notes === undefined ? "" : text(op.notes, "notes", LIMITS.notes);
    const branch = op.branch === undefined ? "" : text(op.branch, "branch", LIMITS.branch);
    if (op.status !== undefined && !isStatus(op.status)) throw new Error('status must be "todo", "doing" or "done".');
    let list = tasks;
    if (list.length >= LIMITS.tasks) {
      // Make room by dropping the oldest finished task. Without one, the user has to clear something.
      const oldestDone = list.filter((t) => t.status === "done").sort((a, b) => a.updatedAt - b.updatedAt)[0];
      if (!oldestDone) throw new Error(`A project holds up to ${LIMITS.tasks} tasks and none is done. Finish or remove one first.`);
      list = list.filter((t) => t !== oldestDone);
    }
    const task: Task = { id: newId(new Set(tasks.map((t) => t.id))), title, status: op.status ?? "todo", createdAt: now, updatedAt: now };
    if (notes) task.notes = notes;
    if (branch) task.links = { branch };
    return { tasks: [...list, task], task };
  }

  const index = tasks.findIndex((t) => t.id === op.id);
  if (index < 0) throw new Error(`No task with id ${String(op.id)}.`);
  if (op.kind === "remove") return { tasks: tasks.filter((t) => t.id !== op.id), task: tasks[index] };

  const next: Task = { ...tasks[index] };
  if (op.title !== undefined) {
    const title = text(op.title, "title", LIMITS.title);
    if (!title) throw new Error("title can't be empty.");
    next.title = title;
  }
  if (op.notes !== undefined) {
    const notes = text(op.notes, "notes", LIMITS.notes);
    if (notes) next.notes = notes;
    else delete next.notes;
  }
  if (op.branch !== undefined) {
    const branch = text(op.branch, "branch", LIMITS.branch);
    if (branch) next.links = { ...next.links, branch };
    else delete next.links;
  }
  if (op.status !== undefined) {
    if (!isStatus(op.status)) throw new Error('status must be "todo", "doing" or "done".');
    next.status = op.status;
  }
  next.updatedAt = now;
  const list = tasks.slice();
  list[index] = next;
  return { tasks: list, task: next };
}

/** Open tasks for the short list and the project section: doing first, then to do, each oldest first. */
export function openTasks(tasks: Task[]): Task[] {
  const order = { doing: 0, todo: 1, done: 2 } as const;
  return tasks.filter((t) => t.status !== "done").sort((a, b) => order[a.status] - order[b.status] || a.createdAt - b.createdAt);
}

// ---------- the Board block ----------

/** What the block's state keeps. `hideDone` leaves the Done column out. */
export interface BoardBlockState {
  hideDone?: boolean;
}

/** Block state from `wings.blockState()`, which is any JSON the page once saved. */
export function cleanBlockState(raw: unknown): BoardBlockState {
  return (raw as Partial<BoardBlockState> | null)?.hideDone === true ? { hideDone: true } : {};
}

/** The board's columns, each in the order the Tasks tab shows: oldest first, and done newest first. */
export function boardColumns(tasks: Task[], hideDone = false): { status: Status; tasks: Task[] }[] {
  return STATUSES.filter((s) => !(hideDone && s === "done")).map((status) => ({
    status,
    tasks: tasks.filter((t) => t.status === status).sort((a, b) => (status === "done" ? b.updatedAt - a.updatedAt : a.createdAt - b.createdAt)),
  }));
}
