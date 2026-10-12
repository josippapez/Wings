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
