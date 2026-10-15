// Notifications about new pull requests: how often the lists are checked while Wings is in the background, and which
// PRs in a list are new. No `wings` is touched until the setting is read, so node tests drive it.
import { setting } from "wings-plugin-sdk/settings";

/** The minutes you can pick between background checks, set in Settings; 0 is off. */
export const WATCH_MINUTES = [0, 5, 10, 15, 30] as const;
export type WatchMinutes = (typeof WATCH_MINUTES)[number];
/** The storage key the settings page saves the minutes under. */
const WATCH_KEY = "watch-minutes";
/** More new PRs than this at once are one notification, so a burst stays under the host's 20 a minute. */
export const MAX_SINGLE = 3;

/** A saved interval, or undefined when what was saved isn't one, which leaves the setting off. */
export function watchOf(saved: unknown): WatchMinutes | undefined {
  return WATCH_MINUTES.find((m) => m === saved);
}

/** How often the lists are checked while Wings is in the background. Settings sets it and the main script hears the change. */
export const watchMinutes = setting<WatchMinutes>(WATCH_KEY, { default: 0, check: watchOf });

/**
 * The PRs in `items` that weren't open on the last check, leaving out yours. A repo listed for the first time has none,
 * since its first list is where counting starts.
 */
export function newPulls<T extends { link: { url: string }; mine: boolean }>(before: ReadonlySet<string> | undefined, items: T[]): T[] {
  if (!before) return [];
  return items.filter((i) => !i.mine && !before.has(i.link.url));
}

type Action = { label: string; open: { view: string; args?: unknown } } | { label: string; url: string };

/**
 * The notifications for a project's new PRs: one each, with buttons to open it in a tab or the browser, or one for all
 * of them when there are more than `MAX_SINGLE`. Every one can open the project's list of open PRs.
 */
export function notesFor(project: string, items: { title: string; author: string | null; url: string; args: unknown }[]): { title: string; body: string; actions: Action[] }[] {
  const all: Action = { label: "All pull requests", open: { view: "pr" } };
  if (items.length > MAX_SINGLE) return [{ title: `${items.length} new pull requests in ${project}`, body: items.map((i) => i.title).join("\n"), actions: [all] }];
  return items.map((i) => ({
    title: `New pull request in ${project}`,
    body: i.author ? `${i.title} · ${i.author}` : i.title,
    actions: [{ label: "Open", open: { view: "pr", args: i.args } }, { label: "Open in browser", url: i.url }, all],
  }));
}
