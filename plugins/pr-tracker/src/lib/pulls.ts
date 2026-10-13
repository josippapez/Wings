// The Pull requests block on a Project page: the messages between the block's page and the main script, and the
// order of its rows. No `wings`, no I/O, so node tests drive it.

import type { PrArgs } from "./types";

/** The most rows a block asks for. A broadcast is limited to 64 KB, and 50 rows of this shape are about 15 KB. */
export const BLOCK_ROWS = 50;

export interface PullRow {
  id: string;
  title: string;
  /** `#12 · repo`, then what needs attention, like failing checks. */
  detail: string;
  /** You wrote it; otherwise it's waiting for your review. */
  mine: boolean;
  badge: { text: string; tone: string };
  /** What the "pr" view is opened with. */
  args: PrArgs;
}

/** The block's page asks main for a project's open pull requests. */
export interface PullsRequest {
  type: "pulls-request";
  rid: string;
  path: string;
}

/** The block asks main to sign in, add a token or install a CLI for the project, which main does as the pane badge's button does. */
export interface PullsAction {
  type: "pulls-action";
  path: string;
}

/** Main says the sign-in finished, so a block asks again instead of waiting for its poll. */
export interface PullsChanged {
  type: "pulls-changed";
}

/** What a block shows when the project's host isn't set up, and the label of the button that sets it up. */
export interface PullsProblem {
  message: string;
  action: "Sign in" | "Add token" | "Install";
}

/** `none` is a project with no remote of a provider the tracker knows, with its host when there is one. */
export interface PullsResponse {
  type: "pulls-response";
  rid: string;
  rows?: PullRow[];
  none?: true;
  /** With `none`: the host nothing recognised, which Check again looks up once more. */
  host?: string;
  problem?: PullsProblem;
  error?: string;
}

/** Whether a cached list of this age may be served: any age for a redraw from pane changes (`anyAge`), else only while it's younger than its TTL. */
export function serveCached(ageMs: number, ttlMs: number, anyAge: boolean): boolean {
  return anyAge || ageMs < ttlMs;
}

/** Yours first, then the ones waiting for your review, each group in the order the list gave. */
export function orderPulls<T extends { mine: boolean }>(rows: T[]): T[] {
  return [...rows.filter((r) => r.mine), ...rows.filter((r) => !r.mine)];
}
