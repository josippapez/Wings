// What the Schedules page and main say to each other. Main is the only writer of the schedules, so a page asks for a change
// and reads the result as a query.

import { channel } from "wings-plugin-sdk/messages";
import type { Draft, Schedule } from "./schedule";

export interface State {
  schedules: Schedule[];
  projects: { id: string; name: string }[];
}

export const ch = channel<
  {
    /** Adds a schedule, or changes the one `id` names. */
    save: { request: { id: string | null; draft: Draft }; response: void };
    toggle: { request: { id: string }; response: void };
    remove: { request: { id: string }; response: void };
    /** Starts the agent now, without moving the next run. Resolves once its tab is open. */
    runNow: { request: { id: string }; response: void };
  },
  {},
  { state: { key: void; value: State } }
>();
