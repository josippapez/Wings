// Pull requests that left a repo's open list: which ones, and what a forge's status says became of them. The tracker only
// lists open PRs, so a merge shows up as a disappearance and the final state is asked for once. No `wings`, no I/O, so
// node tests drive it.

import type { Link, ListedPr } from "./types";

/** An open PR as the last list had it, with what is needed to look it up and tell Issues about it. */
export interface Tracked {
  link: Link;
  title: string;
  /** The source branch. */
  head: string;
  /** The project whose list had it. */
  projectId: string;
  cwd: string;
  /** Lookups of its final state that failed so far. */
  tries: number;
}

/** What `pr-state` carries: a tracked PR that merged or was closed. */
export interface PrStateEvent {
  url: string;
  state: "merged" | "closed";
  sourceBranch: string;
  repo: string;
  provider: Link["provider"];
  number: number;
  projectId: string;
}

/** A failed lookup is tried again on the next lists, this many times in all. */
export const MAX_TRIES = 3;

/**
 * The PRs of `before` that aren't in the new list `items` (they merged, closed or were deleted), and what the next
 * check compares against: this list's PRs. `before` undefined is a repo listed for the first time, which has none gone.
 */
export function settle(before: ReadonlyMap<string, Tracked> | undefined, items: ListedPr[], where: { projectId: string; cwd: string }): { next: Map<string, Tracked>; gone: Tracked[] } {
  const next = new Map(items.map((i): [string, Tracked] => [i.link.url, { link: i.link, title: i.title, head: i.head, tries: 0, ...where }]));
  const gone = [...(before?.values() ?? [])].filter((t) => !next.has(t.link.url));
  return { next, gone };
}

/** The state a status icon says a PR is in, or null for one that is still open. */
export function finalState(icon: string): PrStateEvent["state"] | null {
  return icon === "pr-merged" ? "merged" : icon === "pr-closed" ? "closed" : null;
}

export function eventFor(t: Tracked, state: PrStateEvent["state"]): PrStateEvent {
  return { url: t.link.url, state, sourceBranch: t.head, repo: t.link.repo, provider: t.link.provider, number: t.link.number, projectId: t.projectId };
}

/** Puts a PR whose lookup failed back into the next check's list, until it has been tried `MAX_TRIES` times. */
export function retry(next: Map<string, Tracked>, t: Tracked) {
  if (t.tries + 1 < MAX_TRIES && !next.has(t.link.url)) next.set(t.link.url, { ...t, tries: t.tries + 1 });
}
