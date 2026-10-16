// An agent's own step list for a piece of work: one list per Claude session, with the earlier versions kept. Nothing here touches Wings.

export type TodoStatus = "pending" | "in_progress" | "done";
export interface TodoItem {
  text: string;
  status: TodoStatus;
}
export interface TodoVersion {
  at: number;
  items: TodoItem[];
}
/** One session's list. `versions` is oldest first and its last entry is the list as it stands. */
export interface TodoList {
  sessionId: string;
  sessionName?: string;
  projectId: string;
  /** The issue the list belongs to. */
  issueKey?: string;
  updatedAt: number;
  versions: TodoVersion[];
}

export const TODO_LIMITS = { items: 30, text: 200, versions: 50 } as const;
export const TODO_STATUSES: TodoStatus[] = ["pending", "in_progress", "done"];
export const TODO_MARK: Record<TodoStatus, string> = { done: "[x]", in_progress: "[~]", pending: "[ ]" };

export const itemsOf = (list: TodoList): TodoItem[] => list.versions.at(-1)?.items ?? [];

export function counts(items: TodoItem[]): { done: number; total: number } {
  return { done: items.filter((i) => i.status === "done").length, total: items.length };
}
/** The step being worked on: the first one in progress, else the first not started. */
export const currentStep = (items: TodoItem[]): TodoItem | undefined => items.find((i) => i.status === "in_progress") ?? items.find((i) => i.status === "pending");
/** How many steps are not done. */
export const stepsLeft = (list: TodoList): number => itemsOf(list).filter((i) => i.status !== "done").length;

/** `list` with `items` as its newest version, or `list` itself when they are the list's items already. */
export function withItems(list: TodoList | null, who: { sessionId: string; sessionName?: string; projectId: string }, items: TodoItem[], issueKey: string | undefined, now: number): TodoList {
  const base: TodoList = list ?? { ...who, updatedAt: now, versions: [] };
  const same = JSON.stringify(itemsOf(base)) === JSON.stringify(items);
  if (list && same && (issueKey ?? list.issueKey) === list.issueKey) return list;
  const versions = same ? base.versions : [...base.versions, { at: now, items }].slice(-TODO_LIMITS.versions);
  return { ...base, ...(who.sessionName ? { sessionName: who.sessionName } : {}), ...(issueKey ? { issueKey } : {}), updatedAt: now, versions };
}

/** The list as text for Claude. */
export function formatTodo(list: TodoList | null): string {
  const items = list ? itemsOf(list) : [];
  if (!items.length) return "No todo list for this session yet. Send the steps as items.";
  const { done, total } = counts(items);
  return [`Todo for this session${list!.issueKey ? ` (${list!.issueKey})` : ""}: ${done} of ${total} done.`, ...items.map((i) => `${TODO_MARK[i.status]} ${i.text}`)].join("\n");
}

/** What the pane's card shows: nothing once every step is done. */
export function railOf(list: TodoList, view: string): { title: string; items: TodoItem[]; open: string } | null {
  const items = itemsOf(list);
  if (!items.some((i) => i.status !== "done")) return null;
  const { done, total } = counts(items);
  return { title: `Todo · ${done}/${total}${list.issueKey ? ` · ${list.issueKey}` : ""}`.slice(0, 40), items, open: view };
}
