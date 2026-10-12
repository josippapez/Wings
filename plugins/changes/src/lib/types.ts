/** What the main script and the pages send each other. */

export type Mode = "uncommitted" | "branch";
export type Tab = "changes" | "history";

/** A changed file in a broadcast: `[status, path, added, removed, oldPath]`. */
export type FileRow = [status: string, path: string, added: number | null, removed: number | null, oldPath: string | null];
/** A commit in a broadcast: `[hash, subject, author, at, refs]`, refs as `kind:name`. */
export type CommitRow = [hash: string, subject: string, author: string, at: number, refs: string[]];

export interface Repo {
  root: string;
  name: string;
  branch?: string;
}

/** The Changes tab: the files of one repo and mode. */
export interface StateMessage {
  type: "state";
  mode: Mode;
  loading?: boolean;
  repo: Repo | null;
  against?: string;
  files?: FileRow[];
  total?: number;
  added?: number;
  removed?: number;
  partial?: boolean;
  error?: string;
}

/** The History tab: a page of commits for one search and scope. */
export interface HistoryMessage {
  type: "history";
  query: string;
  all: boolean;
  skip: number;
  repo: Repo | null;
  rows?: CommitRow[];
  more?: boolean;
  error?: string;
}

/** The repo the graph view shows. */
export interface GraphRepoMessage {
  type: "graph-repo";
  root: string | null;
  name: string | null;
}
