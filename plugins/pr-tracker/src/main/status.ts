/** Loading a PR's status, the details of its failed checks, and its diff, from whichever forge it's on. */
import { errorMessage } from "wings-plugin-sdk/async";
import { forgeOf } from "../lib/forges/registry";
import { noLog } from "../lib/forges/shared";
import type { Diff, FailureDetail, Status } from "../lib/types";
import { render } from "./badges";
import { diffLoads, diffs, failKey, failures, inFlight, linkFor, manual, status } from "./state";

// ---------- status ----------

export function refreshStatus(url: string, { byUser = false } = {}): Promise<void> {
  const link = linkFor(url);
  if (!link) return Promise.resolve();
  if (byUser) manual.add(url);
  if (inFlight.has(url)) return inFlight.get(url)!;
  const run = (async () => {
    try {
      status.set(url, await forgeOf(link).status(link));
    } catch (error) {
      status.set(url, { error: errorMessage(error) });
    } finally {
      inFlight.delete(url);
      manual.delete(url);
      render();
    }
    prefetch(url);
  })();
  inFlight.set(url, run);
  render();
  return run;
}

/** After a status loads: the diff, so "View changes" opens at once, and the details of checks that newly failed. */
export function prefetch(url: string) {
  const st = status.get(url);
  if (st && !st.error && diffs.get(url)?.key !== st.key) void loadDiff(url).catch(() => {});
  if (st?.failing?.length && failures.get(url)?.key !== failKey(st as Status)) void loadFailures(url);
}

// ---------- failed checks ----------

/** Details of the PR's failed checks, fetched once per set of failures rather than on every poll. */
export function loadFailures(url: string) {
  const st = status.get(url) as Status;
  const key = failKey(st);
  const cached = failures.get(url);
  if (cached?.key === key) return cached.items;
  const link = linkFor(url);
  // The pane moved off this PR while its status loaded.
  if (!link) return Promise.resolve([]);
  const forge = forgeOf(link);
  // Each check's own load is kept too, so a tab that opens a failed check reads what the badge already fetched.
  const byId = new Map(st.failing.map((f) => [f.id, forge.failure(link, f)]));
  const items = Promise.all(
    st.failing.map((f) => byId.get(f.id)!.catch((error: unknown): FailureDetail[] => noLog(f.name, `${f.summary ?? "Failed"} (no log: ${errorMessage(error)})`, f.url))),
  ).then((lists) => lists.flat());
  const entry = { key, items, byId, list: null as FailureDetail[] | null };
  failures.set(url, entry);
  void items.then((list) => {
    entry.list = list;
    render();
  });
  return items;
}

// ---------- diff and comments ----------

/** The PR's diff and review comments, cached per PR version; concurrent callers share one fetch. */
export function loadDiff(url: string): Promise<Diff> {
  const st = status.get(url);
  const cached = diffs.get(url);
  if (cached && cached.key === st?.key) return Promise.resolve(cached);
  if (diffLoads.has(url)) return diffLoads.get(url)!;
  const link = linkFor(url);
  // The pane moved off this PR while its status loaded.
  if (!link) return Promise.reject(new Error("This pull request isn't open in a pane any more."));
  if (!st || st.error !== undefined) return Promise.reject(new Error(st?.error ?? "The pull request's status hasn't loaded."));
  const load = (async () => {
    const entry = await forgeOf(link).diff(link, st);
    diffs.set(url, { key: st.key, ...entry });
    return diffs.get(url)!;
  })().finally(() => diffLoads.delete(url));
  diffLoads.set(url, load);
  return load;
}
