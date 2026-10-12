/** Loading a PR's status, the details of its failed checks, and its diff, from whichever provider it's on. */
import { azureChecks, azureDiff, azureFailure } from "./azure";
import { githubDiff, githubFailure, githubStatus } from "./github";
import { gitlabDiff, gitlabFailure, gitlabStatus } from "./gitlab";
import { diffLoads, diffs, failKey, failures, inFlight, linkFor, manual, status } from "./state";
import type { AzureLink, Comment, Diff, FailureDetail, Failing, GitlabLink, Link, Status } from "./types";
import { render } from "./view";

// ---------- status ----------

export function refreshStatus(url: string, { byUser = false } = {}): Promise<void> {
  const link = linkFor(url);
  if (!link) return Promise.resolve();
  if (byUser) manual.add(url);
  if (inFlight.has(url)) return inFlight.get(url)!;
  const run = (async () => {
    try {
      const load = {
        github: () => githubStatus(link),
        azure: () => azureChecks(link as AzureLink),
        gitlab: () => gitlabStatus(link as GitlabLink),
      }[link.provider];
      status.set(url, await load());
    } catch (error) {
      status.set(url, { error: String((error as Error).message ?? error) });
    } finally {
      inFlight.delete(url);
      manual.delete(url);
      render();
    }
    const st = status.get(url);
    if (st && !st.error && diffs.get(url)?.key !== st.key) void loadDiff(url).catch(() => {});
    if (st?.failing?.length && failures.get(url)?.key !== failKey(st as Status)) void loadFailures(url);
  })();
  inFlight.set(url, run);
  render();
  return run;
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
  const load = { github: githubFailure, azure: azureFailure, gitlab: gitlabFailure }[link.provider] as (link: Link, f: Failing) => Promise<FailureDetail[]>;
  const items = Promise.all(
    st.failing.map((f) =>
      load(link, f).catch((error: Error): FailureDetail[] => [{ check: f.name, summary: `${f.summary ?? "Failed"} (no log: ${String(error.message ?? error)})`, log: "", url: f.url }]),
    ),
  ).then((lists) => lists.flat());
  const entry = { key, items, list: null as FailureDetail[] | null };
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
  const key = status.get(url)?.key;
  const cached = diffs.get(url);
  if (cached && cached.key === key) return Promise.resolve(cached);
  if (diffLoads.has(url)) return diffLoads.get(url)!;
  const link = linkFor(url);
  // The pane moved off this PR while its status loaded.
  if (!link) return Promise.reject(new Error("This pull request isn't open in a pane any more."));
  const load = (async () => {
    const entry = await ({ github: githubDiff, azure: azureDiff, gitlab: gitlabDiff }[link.provider] as (link: Link) => Promise<{ patch: string; comments: Comment[] }>)(
      link,
    );
    diffs.set(url, { key, ...entry });
    return diffs.get(url)!;
  })().finally(() => diffLoads.delete(url));
  diffLoads.set(url, load);
  return load;
}
