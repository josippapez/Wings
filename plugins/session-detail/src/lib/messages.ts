import { channel } from "wings-plugin-sdk/messages";

import type { Item, SessionView } from "./types";

/** What main and the sidebar say to each other. */
export const ch = channel<
  {
    /** Opens the diff of a file the session edited, or of everything changed when `path` is null. */
    diff: { request: { sessionId: string; path: string | null }; response: void };
  },
  {},
  {
    /** The sessions in panes, as the sidebar lists them. */
    sessions: { key: void; value: SessionView[] };
    /** A row's inputs and result, which the sessions leave out. */
    detail: { key: { sessionId: string; key: string }; value: Item };
  }
>();
