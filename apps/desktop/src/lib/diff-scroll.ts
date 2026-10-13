/** Pixels per row the diff library assumes before it has measured a line (its `DEFAULT_VIRTUAL_FILE_METRICS`). */
export const LINE_HEIGHT = 20;

/** Height of a "N unmodified lines" separator row between hunks (measured in the installed @pierre/diffs 1.5.2). */
export const SEPARATOR_HEIGHT = 32;

export type RowFile = {
  hunks: {
    collapsedBefore: number;
    additionStart: number;
    deletionStart: number;
    hunkContent: ({ type: "context"; lines: number } | { type: "change"; deletions: number; additions: number })[];
  }[];
};

/**
 * The `data-line` a row carries in the diff library's markup for line `line` on `side`, and whether the row is a deleted line.
 * Split view numbers each column by its own side. Unified view has one column: added and context rows carry the new number,
 * so an old-side context line is found by the new number it became, and only deleted lines carry an old number.
 */
export function rowNumber(file: RowFile, side: "additions" | "deletions", line: number): { number: number; deleted: boolean } | null {
  for (const hunk of file.hunks) {
    let old = hunk.deletionStart;
    let now = hunk.additionStart;
    for (const block of hunk.hunkContent) {
      if (block.type === "context") {
        const at = side === "additions" ? line - now : line - old;
        if (at >= 0 && at < block.lines) return { number: now + at, deleted: false };
        old += block.lines;
        now += block.lines;
      } else {
        if (side === "deletions" && line >= old && line < old + block.deletions) return { number: line, deleted: true };
        if (side === "additions" && line >= now && line < now + block.additions) return { number: line, deleted: false };
        old += block.deletions;
        now += block.additions;
      }
    }
  }
  return null;
}

/**
 * Distance in px from the first row to the row of `line` on `side`, when each row is a line and each gap between hunks is a separator.
 * A big file draws only the lines near the viewport, so a far line has no element to scroll to; the difference of two of these says how far
 * to scroll from a row that is on screen. Split view puts a change block's deleted and added lines side by side, unified one after the other.
 * Heights the library measures (comment threads, wrapped lines) are not counted, so callers repeat the scroll until the row shows.
 */
export function rowTop(file: RowFile, side: "additions" | "deletions", line: number, split: boolean): number | null {
  let rows = 0;
  let gaps = 0;
  for (const hunk of file.hunks) {
    if (hunk.collapsedBefore > 0) gaps++;
    let old = hunk.deletionStart;
    let now = hunk.additionStart;
    for (const block of hunk.hunkContent) {
      if (block.type === "context") {
        const at = side === "additions" ? line - now : line - old;
        if (at >= 0 && at < block.lines) return (rows + at) * LINE_HEIGHT + gaps * SEPARATOR_HEIGHT;
        rows += block.lines;
        old += block.lines;
        now += block.lines;
      } else {
        const [start, count] = side === "deletions" ? [old, block.deletions] : [now, block.additions];
        if (line >= start && line < start + count) {
          const before = split || side === "deletions" ? 0 : block.deletions;
          return (rows + before + line - start) * LINE_HEIGHT + gaps * SEPARATOR_HEIGHT;
        }
        rows += split ? Math.max(block.deletions, block.additions) : block.deletions + block.additions;
        old += block.deletions;
        now += block.additions;
      }
    }
  }
  return null;
}
