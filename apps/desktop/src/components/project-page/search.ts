import type { Info } from "@/lib/project-info";

/** One page entry that matches: a fact (by key) or an item (by id). */
export type Match = { kind: "fact"; key: string } | { kind: "item"; id: string };

export const matchId = (m: Match) => (m.kind === "fact" ? `fact:${m.key}` : `item:${m.id}`);

/**
 * Entries whose text holds `query`, case-insensitive, plain text, in page order (facts, then items). Facts match on key
 * and value; sections on title and Markdown source; blocks on their block id. A blank query matches nothing.
 */
export function findMatches(info: Info, query: string): Match[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const has = (...texts: string[]) => texts.some((t) => t.toLowerCase().includes(q));
  return [
    ...info.facts.filter((f) => has(f.key, f.value)).map((f): Match => ({ kind: "fact", key: f.key })),
    ...info.items.filter((i) => (i.kind === "section" ? has(i.title, i.body) : has(i.blockId))).map((i): Match => ({ kind: "item", id: i.id })),
  ];
}

/** The match Enter (`dir` 1) or Shift+Enter (-1) goes to from `at`, wrapping around; 0 when there are none. */
export const stepMatch = (at: number, dir: 1 | -1, count: number) => (count === 0 ? 0 : (at + dir + count) % count);

/** Sent on `window` with the project's id when Find runs while its Project page is shown. */
export const PROJECT_PAGE_FIND = "project-page-find";

/** How a search match shows: any match is tinted, the one Enter is on more strongly. */
export type Mark = "hit" | "current";
export const markClass = (mark: Mark | undefined) => (mark === "current" ? "bg-primary/20 ring-2 ring-primary/60" : mark === "hit" ? "bg-primary/10" : "");

/** `at` kept inside 0..count-1, for when matches shrink under it (an edit while searching). */
export const clampMatch = (at: number, count: number) => Math.max(0, Math.min(at, count - 1));
