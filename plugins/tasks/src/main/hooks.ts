// The two Claude Code hooks: what a session is told as it starts, and the check that keeps issue keys out of commits and PRs.

import { keepLocal } from "../lib/local-setting";
import { localOnlyDenial, writesToHistory } from "../lib/local-only";
import { readStore } from "../lib/storage";
import { sessionStartText } from "../lib/session-start";
import { readTodo } from "../lib/todo-store";
import { stepsLeft } from "../lib/todo";

/** The key prefix of each project that has issues, as main last saw it. */
const prefixes = new Map<string, string>();
export const rememberPrefix = (projectId: string, prefix: string, issues: number) => void (issues ? prefixes.set(projectId, prefix) : prefixes.delete(projectId));

async function sessionStart(input: WingsHookInput, context: WingsToolContext) {
  if (!context.project) return null;
  const { id, name } = context.project;
  const [{ issues }, local, todo] = await Promise.all([readStore(id, name), keepLocal.get(), readTodo(id, input.session_id)]);
  const issue = issues.filter((i) => i.sessions.some((s) => s.id === input.session_id)).sort((a, b) => b.updatedAt - a.updatedAt)[0];
  return sessionStartText({ projectName: name, keepLocal: local, issue, todoLeft: todo ? stepsLeft(todo) : 0 });
}

async function commandCheck(input: WingsHookInput, context: WingsToolContext) {
  const command = input.tool_input?.command;
  if (typeof command !== "string" || !writesToHistory(command) || !(await keepLocal.get())) return null;
  if (context.project && !prefixes.has(context.project.id)) {
    const { prefix, issues } = await readStore(context.project.id, context.project.name);
    rememberPrefix(context.project.id, prefix, issues.length);
  }
  const reason = localOnlyDenial(command, [...new Set(prefixes.values())]);
  return reason ? { deny: reason } : null;
}

export function registerHooks(): void {
  wings.onHook("SessionStart", sessionStart);
  wings.onHook("PreToolUse", commandCheck);
}
