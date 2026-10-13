/** Where a plugin wants the diff viewer to look: `openDiff` / `updateDiff` take it as `focus`. No DOM, so node tests drive it. */

export type DiffFocus = {
  /** The file, as the patch names it (for a renamed file, the new or the old name). */
  path: string;
  /** A line of that file. Without one, or with one the patch doesn't show, the viewer just opens the file. */
  line?: number;
  /** `new` (the default) counts lines of the file after the change, `old` before it. */
  side?: "old" | "new";
};

export type FocusFile = {
  name: string;
  prevName?: string;
  hunks: { additionStart: number; additionCount: number; deletionStart: number; deletionCount: number }[];
};

export type ResolvedFocus = {
  /** Index in the patch's files. */
  index: number;
  /** Set only when the line sits inside a hunk, so there is something to highlight. */
  line?: number;
  side: "additions" | "deletions";
};

/** The file and line of the patch that `focus` points at, or null when the patch has no such file. */
export function resolveFocus(files: FocusFile[], focus: DiffFocus | undefined): ResolvedFocus | null {
  if (!focus || !focus.path) return null;
  const index = files.findIndex((f) => f.name === focus.path);
  const at = index !== -1 ? index : files.findIndex((f) => f.prevName === focus.path);
  if (at === -1) return null;
  const side = focus.side === "old" ? "deletions" : "additions";
  const { line } = focus;
  if (!Number.isInteger(line) || (line as number) < 1) return { index: at, side };
  const shown = files[at].hunks.some((h) => {
    const [start, count] = side === "additions" ? [h.additionStart, h.additionCount] : [h.deletionStart, h.deletionCount];
    return (line as number) >= start && (line as number) < start + count;
  });
  return shown ? { index: at, line, side } : { index: at, side };
}

/** Keeps what a plugin sent if it is a usable focus: a path and, optionally, a line and a side. */
export function cleanFocus(raw: unknown): DiffFocus | undefined {
  const f = raw as { path?: unknown; line?: unknown; side?: unknown } | null | undefined;
  if (!f || typeof f.path !== "string" || !f.path || f.path.length > 500) return undefined;
  return {
    path: f.path,
    ...(Number.isInteger(f.line) && (f.line as number) > 0 ? { line: f.line as number } : {}),
    ...(f.side === "old" || f.side === "new" ? { side: f.side } : {}),
  };
}
