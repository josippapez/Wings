import { boundedJson, VIEW_ARGS_MAX } from "./plugin-pages.ts";

/**
 * A button on a plugin's notification: `open` shows one of the plugin's views as a tab in the notification's project,
 * `url` opens a web page, which Rust checks against the plugin's `openUrl` permission when it's clicked.
 */
export type NotificationButton = { label: string; open: { view: string; args?: unknown } } | { label: string; url: string };

export const NOTIFICATION_BUTTONS_MAX = 3;

/** What the notification list keeps about one thing that happened. */
export type NotificationEvent = {
  id: string;
  /** ms since 1970. */
  at: number;
  /** `needs you` and `finished` are agents, `plugin` is a plugin's own notify call. */
  kind: "blocked" | "done" | "plugin";
  title: string;
  /** The project for an agent, the plugin id for a plugin. */
  project: string;
  /** What the agent was waiting for; empty otherwise. */
  detail: string;
  /** The agent's pane, for Go to pane; null for a plugin or once it can't be found. */
  paneId: string | null;
  read: boolean;
  /** A plugin's notification: the plugin, the project its tabs open in (else the one on screen) and its buttons. */
  plugin?: { id: string; projectId: string | null; buttons: NotificationButton[] };
};

export type NotificationAction =
  | { type: "add"; event: NotificationEvent }
  | { type: "read"; id: string }
  | { type: "readAll" }
  | { type: "clear" };

export const MAX_NOTIFICATIONS = 100;

/** Newest first, never more than 100. Adding an id that's already there changes nothing. */
export function notificationsReducer(list: NotificationEvent[], action: NotificationAction): NotificationEvent[] {
  switch (action.type) {
    case "add":
      return list.some((e) => e.id === action.event.id) ? list : [action.event, ...list].slice(0, MAX_NOTIFICATIONS);
    case "read":
      return list.some((e) => e.id === action.id && !e.read) ? list.map((e) => (e.id === action.id ? { ...e, read: true } : e)) : list;
    case "readAll":
      return list.some((e) => !e.read) ? list.map((e) => (e.read ? e : { ...e, read: true })) : list;
    case "clear":
      return list.length ? [] : list;
  }
}

export const unreadCount = (list: NotificationEvent[]) => list.reduce((n, e) => n + (e.read ? 0 : 1), 0);

/**
 * Plugin data is untrusted: keeps up to 3 buttons with a label of up to 32 characters. Throws when `buttons` isn't a list,
 * or a button opens a view that isn't in `tabViews` (the plugin's views that open as a tab), has args over 16 KB, or
 * has neither a view nor an https link.
 */
export function cleanButtons(buttons: unknown, tabViews: string[]): NotificationButton[] {
  if (buttons === undefined || buttons === null) return [];
  if (!Array.isArray(buttons)) throw new Error("notify actions must be a list");
  return buttons.slice(0, NOTIFICATION_BUTTONS_MAX).map((raw, i) => {
    const b = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
    const label = typeof b.label === "string" ? b.label.trim().slice(0, 32) : "";
    if (!label) throw new Error(`notify action ${i} needs a label`);
    if (typeof b.url === "string") {
      if (!b.url.startsWith("https://") || b.url.length > 2048) throw new Error(`notify action ${label} needs an https link of up to 2048 characters`);
      return { label, url: b.url };
    }
    const open = b.open && typeof b.open === "object" ? (b.open as Record<string, unknown>) : {};
    if (typeof open.view !== "string" || !tabViews.includes(open.view)) {
      throw new Error(`notify action ${label} opens ${String(open.view)}, which isn't one of the plugin's views that open as a tab`);
    }
    const hasArgs = open.args !== undefined && open.args !== null;
    return { label, open: { view: open.view, ...(hasArgs ? { args: JSON.parse(boundedJson(open.args, VIEW_ARGS_MAX, "notify action args")) } : {}) } };
  });
}
