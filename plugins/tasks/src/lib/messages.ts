// What the plugin's frames say to each other. A page asks main to apply an operation, and main tells every page when a
// project's issues changed so they read storage again. A message is a broadcast, limited to 64 KB, and an issue can be
// bigger than that: so the answer is empty and the event carries only the project id.

import { channel } from "wings-plugin-sdk/messages";
import type { Op } from "./domain";

export const ch = channel<
  // `projectName` seeds the key prefix of a project's first issue.
  { op: { request: { projectId: string; projectName: string; op: Op }; response: void } },
  { issues: { projectId: string } }
>();
