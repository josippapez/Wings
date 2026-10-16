import { channel } from "wings-plugin-sdk/messages";

/** A pane's git branch, as main shares it with the sidebar. */
export interface Branch {
  paneId: string;
  project: string;
  branch: string;
}

/** What main and the pages say to each other: main shares the list of branches as `branches`. */
export const ch = channel<{}, {}, { branches: Branch[] }>();
