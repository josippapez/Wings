import { channel } from "wings-plugin-sdk/messages";

import type { ChangesAnswer, CommitRef, HistoryAnswer, Mode } from "./types";

/** How often the badges and an open sidebar check again, in ms, while Wings is in front. */
export const POLL_MS = 15_000;
/** Wings refuses a broadcast over 64,000 characters of JSON, and the envelope an answer travels in takes a few dozen. */
export const MAX_ANSWER = 63_000;

/** What the main script and the pages say to each other. */
export const ch = channel<
  {
    /** The Changes tab: the files of the repo the sidebar follows. */
    changes: { request: { mode: Mode }; response: ChangesAnswer };
    /** A page of the History tab. `skip` is how many rows the list already has. */
    history: { request: { query: string; all: boolean; skip: number }; response: HistoryAnswer };
    /** Opens the diff viewer on one changed file, or on all of them with `path: null`. */
    open: { request: { root: string; mode: Mode; path: string | null }; response: void };
    /** Opens the diff viewer on one commit. */
    commit: { request: CommitRef; response: void };
    /** Opens the graph view on a repo. */
    graph: { request: { root: string }; response: void };
    /** The repo the graph view shows. */
    "graph-repo": { request: void; response: { root: string | null; name: string | null } };
  },
  {
    /** The sidebar's Refresh: main forgets which repo each folder is in, and the page asks again. */
    refresh: void;
    /** The focused pane's repo may have changed, so the sidebar asks again. */
    "repo-changed": void;
    /** The graph view was opened again, maybe on another repo, so it asks which. */
    "graph-repo-changed": void;
  }
>();
