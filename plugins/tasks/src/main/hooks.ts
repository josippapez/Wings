// The hooks: what a session is told as it starts, the check that keeps issue keys out of commits and PRs, and the agent's change journal, which links the turn's commits and notes its files on the session's issue.

import { actorOf } from "../lib/actor";
import { homeOf } from "../lib/home";
import { journalIssue, isLive, journalOf, type JournalInput } from "../lib/journal";
import { keepLocal } from "../lib/local-setting";
import { localOnlyDenial, writesToHistory } from "../lib/local-only";
import { readStore } from "../lib/storage";
import { inReviewText, sessionStartText, staleIssues } from "../lib/session-start";
import { readTodo } from "../lib/todo-store";
import { itemsOf, stepsLeft } from "../lib/todo";
import { write } from "./write";

/** The key prefix of each project that has issues, as main last saw it. */
const prefixes = new Map<string, string>();
export const rememberPrefix = (projectId: string, prefix: string, issues: number) => void (issues ? prefixes.set(projectId, prefix) : prefixes.delete(projectId));

async function sessionStart(input: WingsHookInput, context: WingsToolContext) {
  if (!context.project) return null;
  const { id, name } = await homeOf(context.project);
  const [{ issues }, local, todo] = await Promise.all([readStore(id, name), keepLocal.get(), readTodo(id, input.session_id)]);
  const issue = issues.filter((i) => i.sessions.some((s) => s.id === input.session_id)).sort((a, b) => b.updatedAt - a.updatedAt)[0];
  // A todo with every step done on an issue that is still open is worth a look; the hook only says so.
  const items = todo ? itemsOf(todo) : [];
  const linked = todo?.issueKey ? issues.find((i) => i.key === todo.issueKey) : undefined;
  const finished = items.length && !stepsLeft(todo!) && linked && isLive(linked) ? linked : undefined;
  return sessionStartText({ projectName: name, keepLocal: local, issue, todoLeft: todo ? stepsLeft(todo) : 0, stale: staleIssues(issues, Date.now()), finished });
}

/** Each message in a session: when the session's issue is in review, asks the agent to settle it from what the user says. */
async function promptSubmit(input: WingsHookInput, context: WingsToolContext) {
  if (!context.project) return null;
  const home = await homeOf(context.project);
  const { issues } = await readStore(home.id, home.name);
  const issue = issues.filter((i) => i.sessions.some((s) => s.id === input.session_id)).sort((a, b) => b.updatedAt - a.updatedAt)[0];
  return issue?.status === "in_review" ? inReviewText(issue) : null;
}

/** An agent's journal for a finished turn: links its commits to this session's open issue and records its files in the issue's Files changed. Says nothing back. */
async function journal(input: WingsHookInput, context: WingsToolContext) {
  const sessionId = (input as JournalInput).session_id;
  if (!context.project || typeof sessionId !== "string" || !sessionId) return null;
  const { id, name } = await homeOf(context.project);
  const { files, commits, started } = journalOf(input as JournalInput);
  const issue = journalIssue((await readStore(id, name)).issues, sessionId, started);
  if (!issue) return null;
  // Written by the journal at the end of the turn, not by the agent.
  const by = { ...actorOf(context.session, sessionId), via: "journal" as const };
  // One at a time, and one failing (a full log, say, for a commit link) doesn't stop the others.
  for (const op of [...commits.map((commit) => ({ kind: "update" as const, key: issue.key, commit })), ...(files.length ? [{ kind: "files" as const, key: issue.key, files }] : [])]) {
    await write(id, name, op, by).catch(console.error);
  }
  return null;
}

async function commandCheck(input: WingsHookInput, context: WingsToolContext) {
  const command = input.tool_input?.command;
  if (typeof command !== "string" || !writesToHistory(command) || !(await keepLocal.get())) return null;
  const home = context.project ? await homeOf(context.project) : null;
  if (home && !prefixes.has(home.id)) {
    const { prefix, issues } = await readStore(home.id, home.name);
    rememberPrefix(home.id, prefix, issues.length);
  }
  const reason = localOnlyDenial(command, [...new Set(prefixes.values())]);
  return reason ? { deny: reason } : null;
}

export function registerHooks(): void {
  wings.onHook("SessionStart", sessionStart);
  wings.onHook("PreToolUse", commandCheck);
  wings.onHook("UserPromptSubmit", promptSubmit);
  wings.onHook("Journal", journal);
}
