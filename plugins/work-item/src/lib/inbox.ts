// The pure half of the Updates tab: which events a filter keeps, and the days they group under. No `wings`, no DOM.

import type { Kind, StoredEvent } from "./updates.ts";

export type Filter = "all" | "unread" | "mentions" | "state" | "prs";

export const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "unread", label: "Unread" },
  { value: "mentions", label: "Mentions" },
  { value: "state", label: "State changes" },
  { value: "prs", label: "PRs" },
];

const MENTIONS: Kind[] = ["mention", "pr-mention"];

export function matches(e: StoredEvent, filter: Filter): boolean {
  switch (filter) {
    case "unread": return !e.read;
    case "mentions": return MENTIONS.includes(e.kind);
    case "state": return e.kind === "state";
    case "prs": return e.kind.startsWith("pr-");
    default: return true;
  }
}

export interface Day {
  key: string;
  label: string;
  events: StoredEvent[];
}

const dayKey = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;

/** `events` grouped by the local day they happened on, newest day first and each day's events newest first: "Today", "Yesterday", then the date. */
export function groupByDay(events: StoredEvent[], now = Date.now()): Day[] {
  const today = new Date(now);
  const yesterday = new Date(now);
  yesterday.setDate(today.getDate() - 1);
  const days = new Map<string, Day>();
  for (const e of [...events].sort((a, b) => Date.parse(b.at) - Date.parse(a.at))) {
    const d = new Date(e.at);
    const key = dayKey(d);
    const day = days.get(key) ?? {
      key,
      label: key === dayKey(today) ? "Today" : key === dayKey(yesterday) ? "Yesterday" : d.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" }),
      events: [],
    };
    day.events.push(e);
    days.set(key, day);
  }
  return [...days.values()];
}

/** How many events each filter would show, for the chips. */
export function counts(events: StoredEvent[]): Record<Filter, number> {
  return Object.fromEntries(FILTERS.map((f) => [f.value, events.filter((e) => matches(e, f.value)).length])) as Record<Filter, number>;
}
