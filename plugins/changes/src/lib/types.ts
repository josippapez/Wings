/** What the main script answers the pages with, and what they show. */

export type Mode = "uncommitted" | "branch";

/** A changed file in an answer: `[status, path, added, removed, oldPath]`. */
export type FileRow = [status: string, path: string, added: number | null, removed: number | null, oldPath: string | null];
/** A commit in an answer: `[hash, subject, author, at, refs]`, refs as `kind:name`. */
export type CommitRow = [hash: string, subject: string, author: string, at: number, refs: string[]];

export interface Repo {
  root: string;
  name: string;
  branch: string;
}

/** The Changes tab: the files of one repo and mode, or `repo: null` when no pane is in a repo. */
export type ChangesAnswer =
  | { repo: null }
  | { repo: Repo; against: string; files: FileRow[]; total: number; added: number; removed: number; partial: boolean };

/** One page of the History tab for one search and scope, or `repo: null` when no pane is in a repo. */
export type HistoryAnswer = { repo: null } | { repo: Repo; rows: CommitRow[]; more: boolean };

/** What the History list or the graph already shows of a commit, so the viewer opens with a title before git answers. */
export interface CommitRef {
  root: string;
  hash: string;
  subject: string;
  author: string;
  at: number;
}
