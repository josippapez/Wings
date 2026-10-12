import type { AgentState } from "./api.ts";

/** Where an item comes from. The order is the order of the groups when nothing is typed, and the tie-break when something is. */
export const KINDS = ["agent", "command", "project", "tab", "session", "plugin"] as const;
export type PaletteKind = (typeof KINDS)[number];

export const KIND_LABEL: Record<PaletteKind, string> = {
  agent: "Agents",
  command: "Commands",
  project: "Projects",
  tab: "Tabs",
  session: "Recent sessions",
  plugin: "Plugins",
};

export type PaletteItem = {
  id: string;
  kind: PaletteKind;
  title: string;
  /** Shown beside the title, and searched at half weight. */
  subtitle?: string;
  /** More words to find it by. */
  keywords?: string;
  shortcut?: string;
  /** An agent's state, drawn as its dot. */
  state?: AgentState;
  disabled?: boolean;
  /** The item opens something that takes focus, so focus doesn't go back to the terminal. */
  opensUi?: boolean;
  run: () => void;
};

const isWordStart = (text: string, at: number) => at === 0 || /[\s\-_/.:·]/.test(text[at - 1]);

/**
 * How well `token` matches `text`, or null if it doesn't. A run of the token inside the text beats letters spread
 * over it; the start of the text and the start of a word beat the middle. Both lowercase.
 */
function tokenScore(token: string, text: string): number | null {
  const at = text.indexOf(token);
  if (at !== -1) {
    // A substring, ranked by where it sits: the whole text, the start of it, the start of a word, the middle.
    const base = at === 0 ? (token.length === text.length ? 300 : 200) : isWordStart(text, at) ? 150 : 100;
    return base - Math.min(at, 40) * 0.5 - Math.min(text.length - token.length, 60) * 0.1;
  }
  // Letters in order: each one scores more after the one before it or at the start of a word.
  let score = 10;
  let from = 0;
  let prev = -2;
  for (const ch of token) {
    const i = text.indexOf(ch, from);
    if (i === -1) return null;
    score += i === prev + 1 ? 6 : isWordStart(text, i) ? 4 : 1 - Math.min(i - from, 10) * 0.2;
    prev = i;
    from = i + 1;
  }
  return score;
}

/** Every word of the query has to match, in the title or (at half weight) the subtitle and keywords. */
export function matchScore(query: string, item: Pick<PaletteItem, "title" | "subtitle" | "keywords">): number | null {
  const tokens = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return 0;
  const title = item.title.toLowerCase();
  const rest = `${item.subtitle ?? ""} ${item.keywords ?? ""}`.toLowerCase();
  let total = 0;
  for (const token of tokens) {
    const inTitle = tokenScore(token, title);
    const inRest = tokenScore(token, rest);
    const best = Math.max(inTitle ?? -Infinity, inRest === null ? -Infinity : inRest / 2);
    if (best === -Infinity) return null;
    total += best;
  }
  return total;
}

/** An agent that needs you outranks one that finished, then one that is working, then an idle one. */
const NEED: Record<AgentState, number> = { blocked: 3, done: 2, working: 1, idle: 0 };
const needOf = (item: PaletteItem) => (item.kind === "agent" && item.state ? NEED[item.state] : -1);

/**
 * The items to show. Nothing typed: the agents that need you, then each kind in turn in the order given. Something
 * typed: the items that match, best first, with an agent that needs you ahead of equal matches.
 */
export function rank(items: PaletteItem[], query: string, limit = 80): PaletteItem[] {
  const q = query.trim();
  if (!q) {
    const waiting = items.filter((i) => i.kind === "agent" && needOf(i) >= 2);
    const first = new Set(waiting);
    const kindOrder = (i: PaletteItem) => KINDS.indexOf(i.kind);
    const rest = items.filter((i) => !first.has(i)).sort((a, b) => kindOrder(a) - kindOrder(b));
    return [...waiting.sort((a, b) => needOf(b) - needOf(a)), ...rest].slice(0, limit);
  }
  const scored: { item: PaletteItem; score: number; at: number }[] = [];
  items.forEach((item, at) => {
    const score = matchScore(q, item);
    if (score !== null) scored.push({ item, score: score + needOf(item) * 8, at });
  });
  scored.sort((a, b) => b.score - a.score || KINDS.indexOf(a.item.kind) - KINDS.indexOf(b.item.kind) || a.at - b.at);
  return scored.slice(0, limit).map((s) => s.item);
}

/** The heading an item sits under when nothing is typed. An agent that needs you is in its own group at the top. */
export function groupOf(item: PaletteItem): string {
  return item.kind === "agent" && needOf(item) >= 2 ? "Needs you" : KIND_LABEL[item.kind];
}
