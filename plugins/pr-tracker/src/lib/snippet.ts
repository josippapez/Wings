/** The few lines of a file around a commented range. No I/O. */

export interface SnippetLine {
  n: number;
  text: string;
  /** In the commented range. */
  hit: boolean;
  /** The PR added or changed this line. Set only when the diff was read. */
  added?: boolean;
  /** The PR removed this line, so `n` is its number before the change. Set only on rows read from a diff. */
  removed?: boolean;
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

/**
 * The rows of a unified diff that end at the commented lines, as a host's page shows a review thread: up to 4 rows above
 * the first commented line (context, `+` and `-` alike), then the commented ones. `side` and the numbers say which
 * lines were commented: new-file numbers for `additions`, old-file numbers for `deletions`. Null when the diff has
 * no such line.
 */
export function diffWindow(patch: string, side: "additions" | "deletions", start: number, end: number = start): SnippetLine[] | null {
  interface Row {
    kind: "context" | "added" | "removed";
    text: string;
    old: number;
    now: number;
  }
  const rows: Row[] = [];
  let old = 0;
  let now = 0;
  let inHunk = false;
  const lines = patch.split("\n");
  lines.forEach((line, i) => {
    const header = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)/);
    if (header) {
      [old, now, inHunk] = [Number(header[1]), Number(header[2]), true];
    } else if (line.startsWith("diff ")) {
      inHunk = false;
    } else if (inHunk && line[0] === "+") rows.push({ kind: "added", text: line.slice(1), old, now: now++ });
    else if (inHunk && line[0] === "-") rows.push({ kind: "removed", text: line.slice(1), old: old++, now });
    // A blank context line may have lost its leading space; the patch's last blank line is only its final newline.
    else if (inHunk && (line[0] === " " || (line === "" && i < lines.length - 1))) rows.push({ kind: "context", text: line.slice(1), old: old++, now: now++ });
  });
  const on = (r: Row) => (side === "additions" ? r.kind !== "removed" && r.now >= start && r.now <= end : r.kind !== "added" && r.old >= start && r.old <= end);
  const first = rows.findIndex(on);
  if (first < 0) return null;
  const last = Math.min(rows.findLastIndex(on), first + MAX_RANGE - 1);
  const from = Math.max(0, first - ABOVE);
  return rows.slice(from, last + 1).map((r, i) => ({
    n: r.kind === "removed" ? r.old : r.now,
    text: r.text,
    hit: from + i >= first && on(r),
    ...(r.kind === "added" ? { added: true } : r.kind === "removed" ? { removed: true } : {}),
  }));
}
