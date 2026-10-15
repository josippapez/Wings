import { channel } from "wings-plugin-sdk/messages";

import type { Item, SessionView } from "./types";

/** What main and the sidebar say to each other. */
export const ch = channel<
  {
    /** A row's inputs and result, which the sessions the sidebar lists leave out. */
    detail: { request: { sessionId: string; key: string }; response: Item };
    /** Opens the diff of a file the session edited, or of everything changed when `path` is null. */
    diff: { request: { sessionId: string; path: string | null }; response: void };
  },
  {},
  { sessions: SessionView[] }
>();
