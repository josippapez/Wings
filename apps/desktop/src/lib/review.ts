import type { Agent, PaneInfo, Space } from "./api.ts";

export type ReviewSide = "additions" | "deletions";

/** A comment the user wrote on lines of the diff, to send to an agent. `quote` is the code it points at. */
export type ReviewComment = {
  id: string;
  path: string;
  side: ReviewSide;
  start: number;
  end: number;
  quote: string[];
  body: string;
};

/** The parts of a parsed file diff the quoting needs. */
export type QuotableFile = {
  hunks: { additionStart: number; additionCount: number; additionLineIndex: number; deletionStart: number; deletionCount: number; deletionLineIndex: number }[];
  additionLines: string[];
  deletionLines: string[];
};

const MAX_QUOTED_LINES = 20;

/** The text of line `n` on one side of the diff, or undefined when the diff doesn't show it (collapsed code). */
export function lineText(file: QuotableFile, side: ReviewSide, n: number): string | undefined {
  const add = side === "additions";
  for (const h of file.hunks) {
    const first = add ? h.additionStart : h.deletionStart;
    const count = add ? h.additionCount : h.deletionCount;
    if (n < first || n >= first + count) continue;
    const text = (add ? file.additionLines : file.deletionLines)[(add ? h.additionLineIndex : h.deletionLineIndex) + n - first];
    return text?.replace(/\r?\n$/, "");
  }
  return undefined;
}

/** The lines from `start` to `end` (either order) that the diff shows, at most 20. */
export function quoteLines(file: QuotableFile, side: ReviewSide, start: number, end: number): string[] {
  const lines: string[] = [];
  for (let n = Math.min(start, end); n <= Math.max(start, end) && lines.length < MAX_QUOTED_LINES; n++) {
    const text = lineText(file, side, n);
    if (text !== undefined) lines.push(text);
  }
  return lines;
}

const where = (c: ReviewComment) => {
  const range = c.start === c.end ? `${c.start}` : `${Math.min(c.start, c.end)}-${Math.max(c.start, c.end)}`;
  return `${c.path}:${range}${c.side === "deletions" ? " (removed code)" : ""}`;
};

/** One message for an agent: each comment as `file:line`, the quoted code, then what the reviewer wrote. */
export function formatReview(comments: ReviewComment[], title?: string): string {
  const items = comments.map((c, i) => {
    const quote = c.quote.map((l) => `> ${l}`.trimEnd());
    return [`${i + 1}. ${where(c)}`, ...quote, "", c.body.trim()].join("\n");
  });
  const head = title ? `Review comments on "${title}". Please address each one:` : "Review comments. Please address each one:";
  return [head, ...items].join("\n\n");
}

/** Escapes and other control characters would end the paste early or act as keys, so they never reach the pane. */
export const cleanPaste = (text: string) => text.replace(/\r\n?/g, "\n").replace(/[\x00-\x08\x0b-\x1f\x7f]/g, "");

/** The bytes for a bracketed paste: the pane treats the text as typed-in content and doesn't run it. */
export const bracketedPaste = (text: string) => `\x1b[200~${cleanPaste(text)}\x1b[201~`;

/**
 * Pastes `text` into a pane and, only when `submit` is true, presses Enter in a separate write a moment later,
 * so the agent's input doesn't fold the Enter into the paste.
 */
export async function sendToPane(
  write: (paneId: string, data: string) => Promise<void>,
  paneId: string,
  text: string,
  submit: boolean,
  wait: (ms: number) => Promise<void> = (ms) => new Promise((r) => setTimeout(r, ms)),
) {
  await write(paneId, bracketedPaste(text));
  if (!submit) return;
  await wait(150);
  await write(paneId, "\r");
}

/**
 * A pane the review can go to: only agents. The quoted code comes from the diff, so a plain shell would run it
 * once Enter is pressed.
 */
export type ReviewTarget = { paneId: string; label: string; project: string; cwd: string | null };

const normal = (p: string) => p.replace(/\/+$/, "");
/** `child` is `parent` or a folder inside it. */
export const isInside = (parent: string, child: string) => {
  const a = normal(parent);
  const b = normal(child);
  return b === a || b.startsWith(`${a}/`);
};

/**
 * Every agent as a target, those in `cwd` (the diff's repo) first. `preferred` is the one to preselect: the only
 * agent inside `cwd`, or none.
 */
export function reviewTargets(
  input: {
    agents: Agent[];
    paneInfo: Record<string, PaneInfo>;
    spaces: Space[];
    agentName: (a: Agent) => string;
  },
  cwd?: string,
): { targets: ReviewTarget[]; preferred: string | null } {
  const project = (id: string) => input.spaces.find((s) => s.id === id)?.name ?? "";
  const folder = (paneId: string, spaceId: string) => input.paneInfo[paneId]?.cwd ?? input.spaces.find((s) => s.id === spaceId)?.path ?? null;
  const agents = input.agents.map((a): ReviewTarget => ({ paneId: a.paneId, label: input.agentName(a), project: project(a.spaceId), cwd: folder(a.paneId, a.spaceId) }));
  const near = (t: ReviewTarget) => !!cwd && !!t.cwd && isInside(cwd, t.cwd);
  const ordered = [...agents.filter(near), ...agents.filter((t) => !near(t))];
  const close = agents.filter(near);
  return { targets: ordered, preferred: close.length === 1 ? close[0].paneId : null };
}
