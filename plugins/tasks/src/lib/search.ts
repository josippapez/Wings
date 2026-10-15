// Searching issues.

import type { Issue } from "./domain";

/** What a search looks at, lowercased once per issue: key, title, description and the text of every log entry. */
const haystacks = new WeakMap<Issue, string>();

/** The words of a search, lowercased. */
export const queryWords = (query: string | undefined): string[] => (query ?? "").toLowerCase().split(/\s+/).filter(Boolean);

/** Whether an issue matches a search: every word is somewhere in its key, title, description or log, in any case. No words matches all. */
export function matchesQuery(issue: Issue, query: string | undefined): boolean {
  const words = queryWords(query);
  if (!words.length) return true;
  let text = haystacks.get(issue);
  if (text === undefined) {
    text = [issue.key, issue.title, issue.description ?? "", ...issue.log.map((e) => e.text)].join("\n").toLowerCase();
    haystacks.set(issue, text);
  }
  return words.every((w) => text.includes(w));
}
