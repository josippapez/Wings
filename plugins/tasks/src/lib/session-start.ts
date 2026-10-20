// What Claude is told at the start of a session in a project Issues knows. Short: it goes out every time.

import { STATUS_LABEL, type Issue } from "./domain";
import { isLive } from "./journal";

/** How long an issue in progress or in review may sit unchanged before the session start line mentions it. */
export const STALE_DAYS = 3;
/** The most stale issues the line names. */
const STALE_SHOWN = 3;
const DAY = 24 * 60 * 60 * 1000;

/** The issues in progress or in review untouched for `STALE_DAYS` or more, longest first. */
export const staleIssues = (issues: Issue[], now: number): Issue[] => issues.filter((i) => isLive(i) && now - i.updatedAt >= STALE_DAYS * DAY).sort((a, b) => a.updatedAt - b.updatedAt);

export function sessionStartText(o: { projectName: string; keepLocal: boolean; issue?: Issue; todoLeft?: number; stale?: Issue[]; finished?: Issue; now?: number }): string {
  const lines = [`Wings Issues is the local tracker for ${o.projectName}, kept on this computer for the user and you${o.keepLocal ? ", and its keys never go in commits, branches or PRs" : ""}.`];
  if (o.issue) {
    lines.push(`This session is on ${o.issue.key} "${o.issue.title}" (${STATUS_LABEL[o.issue.status]}). Keep it current: set the status with tasks__update as the work moves and add a tasks__log note before you stop.`);
    if (o.todoLeft) lines.push(`Your todo has ${o.todoLeft} ${o.todoLeft === 1 ? "step" : "steps"} left; tasks__todo with no items reads it.`);
  } else {
    lines.push("Do this first, before any other tool call on a task: find its issue with tasks__list or create one with tasks__create, then write your steps with tasks__todo. Reading or investigating first is no reason to wait, and if you already started without an issue, open it now.");
    if (o.todoLeft) lines.push(`Your todo has ${o.todoLeft} ${o.todoLeft === 1 ? "step" : "steps"} left; tasks__todo with no items reads it.`);
  }
  if (o.finished) lines.push(`Your todo is all done, but ${o.finished.key} is still ${STATUS_LABEL[o.finished.status]}: worth checking whether it is finished.`);
  const stale = (o.stale ?? []).filter((i) => i.key !== o.finished?.key);
  if (stale.length) {
    const now = o.now ?? Date.now();
    const days = (i: Issue) => Math.floor((now - i.updatedAt) / DAY);
    lines.push(`Worth checking: ${stale.slice(0, STALE_SHOWN).map((i) => `${i.key} is ${STATUS_LABEL[i.status]} but unchanged for ${days(i)} days.`).join(" ")}${stale.length > STALE_SHOWN ? ` And ${stale.length - STALE_SHOWN} more.` : ""}`);
  }
  return lines.join("\n");
}
