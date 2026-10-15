// Notifications about new pull requests: how often the lists are checked while Wings is in the background, and which
// PRs in a list are new. No `wings`, so node tests drive it.

/** The minutes you can pick between background checks, set in Settings; 0 is off. */
export const WATCH_MINUTES = [0, 5, 10, 15, 30] as const;
export type WatchMinutes = (typeof WATCH_MINUTES)[number];
/** The storage key the settings page saves the minutes under. */
export const WATCH_KEY = "watch-minutes";
/** More new PRs than this at once are one notification, so a burst stays under the host's 20 a minute. */
export const MAX_SINGLE = 3;

/** A saved interval, or off when what was saved isn't one. */
export function watchOf(saved: unknown): WatchMinutes {
  return WATCH_MINUTES.includes(saved as WatchMinutes) ? (saved as WatchMinutes) : 0;
}

/**
 * The PRs in `items` that weren't open on the last check, leaving out yours. A repo listed for the first time has none,
 * since its first list is where counting starts.
 */
export function newPulls<T extends { link: { url: string }; mine: boolean }>(before: ReadonlySet<string> | undefined, items: T[]): T[] {
  if (!before) return [];
  return items.filter((i) => !i.mine && !before.has(i.link.url));
}

/** The notifications for a project's new PRs: one each, or one for all of them when there are more than `MAX_SINGLE`. */
export function notesFor(project: string, items: { title: string; author: string | null }[]): { title: string; body: string }[] {
  if (items.length > MAX_SINGLE) return [{ title: `${items.length} new pull requests in ${project}`, body: items.map((i) => i.title).join("\n") }];
  return items.map((i) => ({ title: `New pull request in ${project}`, body: i.author ? `${i.title} · ${i.author}` : i.title }));
}
