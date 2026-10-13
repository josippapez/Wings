// What pages and main say to each other. A page's change travels as a broadcast, which Wings limits to 64,000
// characters of JSON, so it carries an operation and ids, and main answers with `info` (not the data): pages read
// the data from storage.

import type { Op } from "./model";

/** A page asks main to change a project's info. `rid` names the request. */
export interface OpMessage {
  type: "op";
  projectId: string;
  rid: string;
  op: Op;
}

/** Main tells every page that a project's info changed. `rid` and `error` answer a page's request. */
export interface InfoMessage {
  type: "info";
  projectId: string;
  rid?: string;
  error?: string;
}
