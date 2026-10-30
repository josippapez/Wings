// Notifications about pull requests: which ones you hear about, how often the lists are checked while Wings is in the
// background, and which PRs in a list are new or newly ask for your review. No `wings` is touched until a setting is
// read, so node tests drive it.
import { setting } from "wings-plugin-sdk/settings";

/** The minutes you can pick between background checks, set in Settings. */
export const WATCH_MINUTES = [5, 10, 15, 30] as const;
export type WatchMinutes = (typeof WATCH_MINUTES)[number];
/** The storage key the settings page saves the minutes under. */
const WATCH_KEY = "watch-minutes";
/** More new PRs than this at once are one notification, so a burst stays under the host's 20 a minute. */
export const MAX_SINGLE = 3;

/** A saved interval, or undefined when what was saved isn't one, which leaves the default. */
export function watchOf(saved: unknown): WatchMinutes | undefined {
  return WATCH_MINUTES.find((m) => m === saved);
}

/** How often the lists are checked while Wings is in the background. Settings sets it and the main script hears the change. */
export const watchMinutes = setting<WatchMinutes>(WATCH_KEY, { default: 5, check: watchOf });

/** What you're notified about: nothing, the PRs that ask for your review, or those and every other new PR. */
export const NOTIFY = ["off", "review", "all"] as const;
export type Notify = (typeof NOTIFY)[number];
export const notifyPulls = setting<Notify>("notify-pulls", { default: "off", check: (v) => NOTIFY.find((n) => n === v) });

/** What the sidebar lists under each project: yours and the ones asked of you, or every open PR. */
export const SIDEBAR = ["mine", "all"] as const;
export type Sidebar = (typeof SIDEBAR)[number];
export const sidebarPulls = setting<Sidebar>("sidebar-pulls", { default: "mine", check: (v) => SIDEBAR.find((n) => n === v) });

/**
 * The PRs in `items` that weren't open on the last check, leaving out yours. A repo listed for the first time has none,
 * since its first list is where counting starts.
 */
export function newPulls<T extends { link: { url: string }; mine: boolean }>(before: ReadonlySet<string> | undefined, items: T[]): T[] {
  if (!before) return [];
  return items.filter((i) => !i.mine && !before.has(i.link.url));
}

/**
 * The PRs in `items` that ask for your review and didn't on the last check: a new PR with you as a reviewer, or an open
 * one you were added to. `before` holds the ones that asked last time, and a repo listed for the first time has none.
 */
export function newReviews<T extends { link: { url: string }; mine: boolean; reviewing: boolean }>(before: ReadonlySet<string> | undefined, items: T[]): T[] {
  if (!before) return [];
  return items.filter((i) => !i.mine && i.reviewing && !before.has(i.link.url));
}

type Action = { label: string; open: { view: string; args?: unknown } } | { label: string; url: string };

/**
 * The notifications for a project's new PRs, or its new review requests with `review`: one each, with buttons to open
 * it in a tab or the browser, or one for all of them when there are more than `MAX_SINGLE`. Every one can open the
 * project's list of open PRs.
 */
export function notesFor(project: string, items: { title: string; author: string | null; url: string; args: unknown }[], review = false): { title: string; body: string; actions: Action[] }[] {
  const all: Action = { label: "All pull requests", open: { view: "pr" } };
  if (items.length > MAX_SINGLE) {
    const title = review ? `${items.length} pull requests to review in ${project}` : `${items.length} new pull requests in ${project}`;
    return [{ title, body: items.map((i) => i.title).join("\n"), actions: [all] }];
  }
  return items.map((i) => ({
    title: review ? `Review asked in ${project}` : `New pull request in ${project}`,
    body: i.author ? `${i.title} · ${i.author}` : i.title,
    actions: [{ label: "Open", open: { view: "pr", args: i.args } }, { label: "Open in browser", url: i.url }, all],
  }));
}
