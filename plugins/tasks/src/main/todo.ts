// Agents' todo lists: the `todo` tool's writes, and the card each list shows in the corner of its agent's pane.

import { serial } from "wings-plugin-sdk/async";
import type { ToolContext } from "wings-plugin-sdk/tools";
import { callerOf } from "../lib/actor";
import { normalizeKey } from "../lib/domain";
import { ch } from "../lib/messages";
import { readStore } from "../lib/storage";
import { readTodo, writeTodo } from "../lib/todo-store";
import { formatTodo, railOf, withItems, type TodoItem, type TodoList } from "../lib/todo";
import { write } from "./write";

/** The Todo view's id: what the card opens. */
const VIEW = "todo";
/** `setPaneRail` is Wings 0.1.54: the SDK's types may not have it yet. */
type Rail = { title: string; items: TodoItem[]; open?: string };
const setRail = (paneId: string, rail: Rail | null): Promise<void> =>
  ((wings as unknown as { setPaneRail?: (paneId: string, rail: Rail | null) => Promise<void> }).setPaneRail?.(paneId, rail) ?? Promise.resolve()).catch(() => {});

/** Lists as last read or written, by session, so a pane change never reads storage twice for one. */
const lists = new Map<string, TodoList | null>();
let panes: WingsPane[] = [];

async function listOf(projectId: string, sessionId: string): Promise<TodoList | null> {
  if (!lists.has(sessionId)) lists.set(sessionId, await readTodo(projectId, sessionId));
  return lists.get(sessionId) ?? null;
}

async function showOn(pane: WingsPane): Promise<void> {
  if (!pane.session) return;
  const list = await listOf(pane.projectId, pane.session.sessionId);
  await setRail(pane.paneId, list ? railOf(list, VIEW) : null);
}

/** Called with every pane on each change: the card follows the session to whichever pane it runs in. */
export function onPaneList(list: WingsPane[]): void {
  panes = list;
  for (const pane of list) showOn(pane).catch(console.error);
}

const queue = serial();

/** Replaces the calling session's list with `items`, or reads it when `items` is left out. */
export async function runTodo(context: ToolContext, items: TodoItem[] | undefined, key: string | undefined): Promise<string> {
  const { projectId, projectName, actor } = callerOf(context);
  const session = context.session;
  if (!session) throw new Error("This call has no agent session yet, so there is no list to keep. Try again in a moment.");
  return queue(async () => {
    const current = await listOf(projectId, session.id);
    if (!items) return formatTodo(current);
    const { issues } = await readStore(projectId, projectName);
    const wanted = key === undefined ? undefined : normalizeKey(key);
    if (wanted && !issues.some((i) => i.key === wanted)) throw new Error(`No issue ${wanted} in ${projectName}. Use tasks__list to see the keys.`);
    // Without a key the list stays with its issue, or joins the one this session is already linked to.
    const linked = wanted ?? current?.issueKey ?? issues.filter((i) => i.sessions.some((x) => x.id === session.id)).sort((a, b) => b.updatedAt - a.updatedAt)[0]?.key;
    const next = withItems(current, { sessionId: session.id, sessionName: session.name ?? undefined, sessionKind: session.kind, projectId }, items, linked, Date.now());
    if (next !== current) {
      await writeTodo(next);
      lists.set(session.id, next);
      await ch.tell("todo", { projectId }).catch(console.error);
      for (const pane of panes) if (pane.session?.sessionId === session.id) await showOn(pane);
    }
    if (wanted) await write(projectId, projectName, { kind: "touch", key: wanted }, actor);
    return formatTodo(next);
  });
}
