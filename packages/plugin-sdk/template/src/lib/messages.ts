import { channel } from "wings-plugin-sdk/messages";

/** A pane's git branch, as main serves it to the sidebar. */
export interface Branch {
  paneId: string;
  project: string;
  branch: string;
}

/** What main and the pages say to each other: main serves the list of branches as the query `branches`. */
export const ch = channel<{}, {}, { branches: { key: void; value: Branch[] } }>();
