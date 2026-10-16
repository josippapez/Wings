// Todo lists in wings.storage: `todo/<slot>/<session id>`, one entry per session, where the slot is the project's, as for issues.
// Only main writes them.

import { slotOf } from "./storage";
import type { TodoList } from "./todo";

const prefix = (projectId: string) => `todo/${slotOf(projectId)}/`;
/** A session id as a storage key part: Wings' ids are UUIDs, and anything else is cleaned to what a key allows. */
const keyOf = (projectId: string, sessionId: string) => prefix(projectId) + sessionId.replace(/[^A-Za-z0-9\-_.:]/g, "_").slice(0, 64);

export async function readTodo(projectId: string, sessionId: string): Promise<TodoList | null> {
  return ((await wings.storage.get(keyOf(projectId, sessionId))) as TodoList | null) ?? null;
}
/** Every list of the project, newest first. */
export async function readTodos(projectId: string): Promise<TodoList[]> {
  return ((await wings.storage.entries(prefix(projectId))) as [string, TodoList][]).map(([, v]) => v).sort((a, b) => b.updatedAt - a.updatedAt);
}
export const writeTodo = (list: TodoList): Promise<void> => wings.storage.set(keyOf(list.projectId, list.sessionId), list);
