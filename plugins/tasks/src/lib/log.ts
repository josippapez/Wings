// The append-only log of an issue and the sessions it records.

import { isAgent } from "./actor";
import { LIMITS, type Actor, type Issue, type LogEntry } from "./domain";

/** Records that `actor`'s session worked on `issue`: adds it, or refreshes its name and time. Users aren't sessions. */
export function touch(issue: Issue, actor: Actor, now: number): void {
  issue.updatedAt = now;
  if (!isAgent(actor) || !actor.id) return;
  const known = issue.sessions.some((s) => s.id === actor.id);
  const about = { ...(actor.name ? { name: actor.name } : {}), ...(actor.kind ? { kind: actor.kind } : {}) };
  issue.sessions = known ? issue.sessions.map((s) => (s.id === actor.id ? { ...s, seen: now, ...about } : s)) : [...issue.sessions, { id: actor.id, ...about, seen: now }];
}

export function append(issue: Issue, entries: LogEntry[]): void {
  if (issue.log.length + entries.length > LIMITS.log) throw new Error(`${issue.key} already has ${LIMITS.log} log entries, the most one issue keeps.`);
  issue.log = [...issue.log, ...entries];
}

/** Like `append`, but drops what doesn't fit instead of throwing: for entries the plugin writes on its own. */
export function appendLoose(issue: Issue, entries: LogEntry[]): void {
  issue.log = [...issue.log, ...entries.slice(0, Math.max(0, LIMITS.log - issue.log.length))];
}
