// What an agent's change journal reports at the end of a turn, as the issue's commit links and files. Nothing here touches Wings.

import type { Issue } from "./domain";

/** One changed file, as the journal describes it from the diff. */
export interface JournalFile {
  path: string;
  what: string;
}
/** The journal's report, as `wings journal` passes it on. Anything may be missing or malformed: it is another tool's output. */
export interface JournalInput {
  session_id?: unknown;
  agent?: unknown;
  /** When the turn's first prompt arrived, as an ISO time. */
  started?: unknown;
  files?: unknown;
  commits?: unknown;
}

const oneLine = (value: string) => value.replace(/\s+/g, " ").trim();

/** The files of a report with a path, and the commit hashes of 7 to 40 hex digits, dropping what isn't. `more` (files beyond those listed) has no row to make. */
export function journalOf(input: JournalInput): { files: JournalFile[]; commits: string[]; started?: number } {
  const files = (Array.isArray(input.files) ? input.files : []).flatMap((f): JournalFile[] => {
    const path = typeof f?.path === "string" ? oneLine(f.path) : "";
    return path ? [{ path, what: typeof f.what === "string" ? oneLine(f.what) : "" }] : [];
  });
  const commits = (Array.isArray(input.commits) ? input.commits : []).filter((c): c is string => typeof c === "string" && /^[0-9a-fA-F]{7,40}$/.test(c));
  const started = typeof input.started === "string" ? Date.parse(input.started) : NaN;
  return { files, commits, ...(Number.isNaN(started) ? {} : { started }) };
}

/**
 * The issue a journal report belongs to. The one the session touched since the turn started, whatever its status, since
 * closing it is often the turn's last step; without `started`, the session's most recently updated one if it is in
 * progress or in review. Either way a child of that issue the session touched as well wins over it, so an epic logged
 * last doesn't take its task's files.
 */
export const journalIssue = (issues: Issue[], sessionId: string, started?: number): Issue | undefined => {
  const seen = (i: Issue) => i.sessions.find((s) => s.id === sessionId)?.seen ?? -1;
  const mine = issues.filter((i) => seen(i) >= 0);
  const pick = (list: Issue[], by: (i: Issue) => number) => {
    const sorted = [...list].sort((a, b) => by(b) - by(a));
    return sorted[0] && (sorted.find((i) => i.parent === sorted[0].key) ?? sorted[0]);
  };
  if (started !== undefined) {
    const turn = pick(mine.filter((i) => seen(i) >= started), seen);
    if (turn) return turn;
  }
  const latest = pick(mine, (i) => i.updatedAt);
  return latest && isLive(latest) ? pick(mine.filter(isLive), (i) => i.updatedAt) : undefined;
};

/** In progress or in review: the statuses that say someone is working on it. */
export const isLive = (issue: Issue): boolean => issue.status === "in_progress" || issue.status === "in_review";
