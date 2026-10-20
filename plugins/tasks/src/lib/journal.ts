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
  files?: unknown;
  commits?: unknown;
}

const oneLine = (value: string) => value.replace(/\s+/g, " ").trim();

/** The files of a report with a path, and the commit hashes of 7 to 40 hex digits, dropping what isn't. `more` (files beyond those listed) has no row to make. */
export function journalOf(input: JournalInput): { files: JournalFile[]; commits: string[] } {
  const files = (Array.isArray(input.files) ? input.files : []).flatMap((f): JournalFile[] => {
    const path = typeof f?.path === "string" ? oneLine(f.path) : "";
    return path ? [{ path, what: typeof f.what === "string" ? oneLine(f.what) : "" }] : [];
  });
  const commits = (Array.isArray(input.commits) ? input.commits : []).filter((c): c is string => typeof c === "string" && /^[0-9a-fA-F]{7,40}$/.test(c));
  return { files, commits };
}

/** The issue a journal report belongs to: the session's most recently updated one, if it is in progress or in review. */
export const journalIssue = (issues: Issue[], sessionId: string): Issue | undefined => {
  const mine = issues.filter((i) => i.sessions.some((s) => s.id === sessionId)).sort((a, b) => b.updatedAt - a.updatedAt)[0];
  return mine && isLive(mine) ? mine : undefined;
};

/** In progress or in review: the statuses that say someone is working on it. */
export const isLive = (issue: Issue): boolean => issue.status === "in_progress" || issue.status === "in_review";
