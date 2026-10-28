// What the Schedules page and main say to each other. Main is the only writer of the schedules, so a page asks for a change
// and reads the result as a query.

import { channel } from "wings-plugin-sdk/messages";
import type { Watch, WatchDraft } from "./assigned";
import type { Draft, Schedule } from "./schedule";

/** An "assigned" entry as the page sees it: without the ids it has already seen. */
/** `targets` are the projects its tickets can start in, its own first. */
export type WatchView = Omit<Watch, "seen"> & { targets: { id: string; name: string }[] };

export interface State {
  schedules: Schedule[];
  watches: WatchView[];
  projects: { id: string; name: string }[];
  /** The one-time code of a GitHub sign-in that is running, which the page shows. */
  signInCode: string | null;
}

export const ch = channel<
  {
    /** Adds a schedule, or changes the one `id` names. */
    save: { request: { id: string | null; draft: Draft }; response: void };
    toggle: { request: { id: string }; response: void };
    remove: { request: { id: string }; response: void };
    /** Starts the agent now, without moving the next run. Resolves once its tab is open. */
    runNow: { request: { id: string }; response: void };
    /** Adds an "assigned" entry, or changes the one `id` names. */
    saveWatch: { request: { id: string | null; draft: WatchDraft }; response: void };
    toggleWatch: { request: { id: string }; response: void };
    removeWatch: { request: { id: string }; response: void };
    /** Looks for new tickets now. */
    checkWatch: { request: { id: string }; response: void };
    /** Makes the worktree and starts the agent for a waiting or listed ticket. Resolves once its tab is open. */
    startTicket: { request: { id: string; ticketId: string; force?: boolean; projectId?: string }; response: void };
    dismissWaiting: { request: { id: string; ticketId: string }; response: void };
    /** Runs the entry's CLI login, or installs the CLI. */
    signIn: { request: { id: string }; response: void };
    install: { request: { id: string }; response: void };
  },
  {},
  { state: { key: void; value: State } }
>();
