/** The few lines of a file around a commented range. No I/O. */

export interface SnippetLine {
  n: number;
  text: string;
  /** In the commented range. */
  hit: boolean;
  /** The PR added or changed this line. Set only when the diff was read. */
  added?: boolean;
}

const ABOVE = 4;
const BELOW = 3;
/** A comment on a long range shows its first lines, not the whole range. */
const MAX_RANGE = 6;

/** The first and last line a snippet of a commented range shows, and its last commented line, before the file's end cuts it. A card holds this room while the file loads. */
export function snippetSpan(start: number, end: number = start): { from: number; to: number; last: number } {
  const last = Math.min(Math.max(end, start), start + MAX_RANGE - 1);
  return { from: Math.max(1, start - ABOVE), to: last + BELOW, last };
}

/** The new-side lines a `diff -U0` adds or changes, read from its hunk headers (`@@ -a,b +c,d @@`). */
export function addedLinesOf(diff: string): Set<number> {
  const added = new Set<number>();
  for (const m of diff.matchAll(/^@@ -\S+ \+(\d+)(?:,(\d+))? @@/gm)) {
    const first = Number(m[1]);
    const count = m[2] === undefined ? 1 : Number(m[2]);
    for (let n = first; n < first + count; n++) added.add(n);
  }
  return added;
}

/** The lines from 4 above the commented range to 3 below it (1-based, inclusive). Empty when the range is past the end of the file. `added`, when given, marks the lines the PR added. */
export function snippetOf(content: string, start: number, end: number = start, added?: Set<number>): SnippetLine[] {
  const lines = content.split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (!(start >= 1) || start > lines.length) return [];
  const span = snippetSpan(start, end);
  const to = Math.min(lines.length, span.to);
  const out: SnippetLine[] = [];
  for (let n = span.from; n <= to; n++) {
    out.push({ n, text: lines[n - 1].replace(/\r$/, ""), hit: n >= start && n <= span.last, ...(added ? { added: added.has(n) } : {}) });
  }
  return out;
}
