/** Counting check results, and turning a failed check's log into the line that says why and the end of the log. */
import type { Counts, FailureDetail, Result } from "./types";

export function counts(results: Result[]) {
  const c: Counts = { passed: 0, failed: 0, running: 0 };
  for (const r of results) c[r]++;
  return c;
}

// ---------- failed checks ----------

const LOG_LINES = 150;
const LOG_CHARS = 8_000;
export const TOOL_CHARS = 30_000;

// These lines say a step failed, not why.
const GENERIC = /\b(exit|exited|exiting|completed|failed) with (exit )?code|^ERROR: Job failed/i;
// A line the tool marked as an error beats one that only mentions errors, like a table header.
const MARKED = /\[(error|fail|failed|fatal)\]|^\s*(error|fatal|failed)\b|\blevel=(error|fatal)\b|^\s*[✖✗×]/i;
const ERRORISH = /error|fail|violation|vulnerab|\[warn\]|panic|exception/i;

/** Log lines without colours, timestamps, GitLab section markers, progress redraws or the CI's `##[...]` markup. */
export function cleanLog(lines: string[]) {
  return lines.map(
    (l) =>
      l
        .replace(/\x1b\[[0-9;?]*[A-Za-z]/g, "")
        .replace(/section_(?:start|end):\d+:[^\r\n]*\r?/g, "")
        .split("\r")
        .filter(Boolean)
        .at(-1)
        ?.replace(/^\d{4}-\d\d-\d\dT[\d:.]+Z ?/, "")
        .replace(/^##\[(error|warning)\]/, "$1: ")
        .replace(/^##\[\w+\]/, "")
        .trimEnd() ?? "",
  );
}

/** The line that best says what went wrong: the CI's own error, else the last line that reads like one. */
function summarize(lines: string[], issues: string[]) {
  const text = lines.filter((l) => l.trim() && !GENERIC.test(l.trim()));
  return (
    issues.find((m) => !GENERIC.test(m)) ??
    text.findLast((l) => MARKED.test(l)) ??
    text.findLast((l) => ERRORISH.test(l)) ??
    text.at(-1) ??
    "Failed"
  ).trim();
}

/** The end of the log, where the error is. */
function tail(lines: string[]) {
  const text = lines.slice(-LOG_LINES).join("\n").trim();
  return text.length > LOG_CHARS ? text.slice(-LOG_CHARS).replace(/^[^\n]*\n/, "") : text;
}

export const detail = (check: string | undefined, step: string | null, lines: string[], issues: string[], url: string | null | undefined): FailureDetail => ({
  check,
  step,
  summary: summarize(lines, issues),
  log: tail(lines),
  url,
});

/** A code fence longer than any backtick run in the text, so the log can't close it and continue as plain text. */
export function fenced(text: string) {
  const fence = "`".repeat(Math.max(3, ...[...text.matchAll(/`+/g)].map((m) => m[0].length + 1)));
  return `${fence}text\n${text}\n${fence}`;
}
