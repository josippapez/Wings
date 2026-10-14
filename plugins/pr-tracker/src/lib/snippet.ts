/** The few lines of a file around a commented range. No I/O. */

export interface SnippetLine {
  n: number;
  text: string;
  /** In the commented range. */
  hit: boolean;
}

const ABOVE = 4;
const BELOW = 3;
/** A comment on a long range shows its first lines, not the whole range. */
const MAX_RANGE = 6;

/** The lines from 4 above the commented range to 3 below it (1-based, inclusive). Empty when the range is past the end of the file. */
export function snippetOf(content: string, start: number, end: number = start): SnippetLine[] {
  const lines = content.split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (!(start >= 1) || start > lines.length) return [];
  const last = Math.min(Math.max(end, start), start + MAX_RANGE - 1, lines.length);
  const from = Math.max(1, start - ABOVE);
  const to = Math.min(lines.length, last + BELOW);
  const out: SnippetLine[] = [];
  for (let n = from; n <= to; n++) out.push({ n, text: lines[n - 1].replace(/\r$/, ""), hit: n >= start && n <= last });
  return out;
}
