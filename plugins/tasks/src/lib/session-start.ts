// What Claude is told at the start of a session in a project Issues knows. Short: it goes out every time.

import { STATUS_LABEL, type Issue } from "./domain";

export function sessionStartText(o: { projectName: string; keepLocal: boolean; issue?: Issue; todoLeft?: number }): string {
  const lines = [`Wings Issues is the local tracker for ${o.projectName}, kept on this computer for the user and you${o.keepLocal ? ", and its keys never go in commits, branches or PRs" : ""}.`];
  if (o.issue) {
    lines.push(`This session is on ${o.issue.key} "${o.issue.title}" (${STATUS_LABEL[o.issue.status]}). Keep it current: set the status with tasks__update as the work moves and add a tasks__log note before you stop.`);
    if (o.todoLeft) lines.push(`Your todo has ${o.todoLeft} ${o.todoLeft === 1 ? "step" : "steps"} left; tasks__todo with no items reads it.`);
  } else {
    lines.push("Do this first, before any other tool call on a task: find its issue with tasks__list or create one with tasks__create, then write your steps with tasks__todo. Reading or investigating first is no reason to wait, and if you already started without an issue, open it now.");
    if (o.todoLeft) lines.push(`Your todo has ${o.todoLeft} ${o.todoLeft === 1 ? "step" : "steps"} left; tasks__todo with no items reads it.`);
  }
  return lines.join("\n");
}
