// The Pull requests block on a Project page: the messages between the block's page and the main script, and the
// order of its rows. No `wings`, no I/O, so node tests drive it.

import type { Counts, PrArgs } from "./types";

/** The most rows a list asks for, which is what a provider's one call returns. */
export const BLOCK_ROWS = 100;
/** A broadcast is limited to 64 KB; a row is 0.5 to 1 KB, so a long list of long titles is cut short rather than refused. */
export const MAX_BYTES = 56 * 1024;

/** Whose a pull request is: you wrote it, your review is asked for, or neither. */
export type Scope = "mine" | "review" | "other";

/** What a list shows: every open PR, yours, the ones asked of you, or drafts. */
export type Filter = "all" | "mine" | "review" | "drafts";

export interface PullRow {
  id: string;
  title: string;
  /** `#12`, or `!12` on GitLab. */
  number: string;
  author: string | null;
  /** `feature/x → main`. */
  branch: string;
  scope: Scope;
  draft: boolean;
  badge: { text: string; tone: string };
  /** The reviewers' verdict in words, "2 approved", or "" where the list doesn't say. */
  review: string;
  /** Check counts, all zero where the list can't see them. */
  checks: Counts;
  /** ISO time. */
  updated: string;
  /** Comments since you last looked. */
  unread: number;
  /** A Claude session in one of your panes is on this PR. */
  agent: "working" | "here" | null;
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

const SCOPE_ORDER: Scope[] = ["mine", "review", "other"];

/** Yours first, then the ones waiting for your review, then the rest, each group in the order the list gave. */
export function orderPulls<T extends { scope: Scope }>(rows: T[]): T[] {
  return SCOPE_ORDER.flatMap((scope) => rows.filter((r) => r.scope === scope));
}

export function filterPulls<T extends { scope: Scope; draft: boolean }>(rows: T[], filter: Filter): T[] {
  if (filter === "all") return rows;
  if (filter === "drafts") return rows.filter((r) => r.draft);
  return rows.filter((r) => r.scope === filter);
}

/** How many rows each filter shows, for its chip. */
export function countPulls(rows: { scope: Scope; draft: boolean }[]): Record<Filter, number> {
  return { all: rows.length, mine: filterPulls(rows, "mine").length, review: filterPulls(rows, "review").length, drafts: filterPulls(rows, "drafts").length };
}

/** A saved filter, or "all" when what was saved isn't one. */
export function filterOf(saved: unknown): Filter {
  const f = (saved as { filter?: unknown } | null)?.filter;
  return f === "mine" || f === "review" || f === "drafts" ? f : "all";
}

/**
 * A title's leading `[tags]` (a ticket, an area) apart from its text, so the page can show them small and muted.
 * Only a title that has text after the tags is split.
 */
export function splitTitle(title: string): { tags: string[]; text: string } {
  const tags: string[] = [];
  let rest = title.trim();
  for (let m = rest.match(/^\[([^\]]{1,40})\]\s*/); m; m = rest.match(/^\[([^\]]{1,40})\]\s*/)) {
    tags.push(m[1].trim());
    rest = rest.slice(m[0].length);
  }
  return rest ? { tags, text: rest } : { tags: [], text: title.trim() };
}

/** The rows that fit a broadcast, in order: stops before the one that would pass `max` bytes. */
export function fitRows<T>(rows: T[], max = MAX_BYTES): T[] {
  let size = 0;
  const out: T[] = [];
  for (const r of rows) {
    size += JSON.stringify(r).length + 1;
    if (size > max) break;
    out.push(r);
  }
  return out;
}
