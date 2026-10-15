/** What the main script knows about the panes' and the lists' pull requests: every map its modules read and write. */
import type { Diff, FailureDetail, Link, Status, StatusEntry } from "../lib/types";

export const state = { panes: [] as WingsPane[] };
/** paneId → PR ref { provider, url, number, repo, ...provider fields } */
export const links = new Map<string, Link>();
/** paneId → { ahead, behind }: commits to push and pull on the pane's branch, as of the last fetch. */
export const sync = new Map<string, { ahead: number; behind: number }>();
/** PR url → normalized status, or { error } */
export const status = new Map<string, StatusEntry>();
/** PR url → its running status refresh. */
export const inFlight = new Map<string, Promise<void>>();
/** PRs refreshed because you clicked Refresh; background polls don't show a spinner. */
export const manual = new Set<string>();
/** PR url → { key, patch, comments }, fetched ahead so "View changes" opens instantly. */
export const diffs = new Map<string, Diff>();
export const diffLoads = new Map<string, Promise<Diff>>();
/** Open diff viewer id → PR url, so a reply typed in it reaches the right PR. */
export const viewers = new Map<string, string>();
/** PR url → { key, items, list }: details of the failed checks `key` names. `items` is the load, `list` its result. */
export const failures = new Map<string, { key: string; items: Promise<FailureDetail[]>; list: FailureDetail[] | null }>();

export const failKey = (st: Status) => st.failing.map((f) => f.id).join(" ");

/** PR url → its ref, for PRs from the project lists and the PR page that no pane is on. */
export const known = new Map<string, Link>();

export function linkFor(url: string) {
  return [...links.values()].find((l) => l.url === url) ?? known.get(url);
}

/** Wings doesn't say when a diff viewer is closed, so only the newest are kept track of. */
const MAX_VIEWERS = 20;

/** What is kept per PR (its status, diff and failed checks) goes once no pane, list or open diff viewer shows the PR. `listed` is the urls of every list. */
export function prune(listed: Iterable<string>) {
  while (viewers.size > MAX_VIEWERS) viewers.delete(viewers.keys().next().value!);
  const shown = new Set([...listed, ...[...links.values()].map((l) => l.url), ...viewers.values()]);
  const kept: Map<string, unknown>[] = [status, diffs, failures];
  for (const map of kept) for (const url of map.keys()) if (!shown.has(url)) map.delete(url);
}
