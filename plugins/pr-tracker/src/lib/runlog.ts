/** A CI log as the run view draws it: colours, the CI's markup and folded groups. No `wings`, no I/O, so node tests drive it. */
import type { CheckState } from "./types";

/** What the CI marked a line as, by its `##[...]` prefix. */
export type Tone = "error" | "warning" | "command" | "section" | "debug" | null;

/** A run of text in one style. `fg` is an ANSI colour, 0 to 15. */
export interface Span {
  text: string;
  fg?: number;
  bold?: true;
  dim?: true;
  italic?: true;
  underline?: true;
}

export interface LogLine {
  n: number;
  /** HH:MM:SS, where the CI stamped the line. */
  time: string | null;
  tone: Tone;
  spans: Span[];
}

/** A line, or a group the CI folds (`##[group]`, GitLab's sections) with its title line. */
export type LogEntry = LogLine | { title: LogLine; lines: LogLine[] };

const STAMP = /^(\d{4}-\d\d-\d\dT(\d\d:\d\d:\d\d)[\d.]*Z) ?/;
const MARK = /^(?:##)?\[(group|endgroup|section|command|error|warning|debug)\]/;
// GitLab's section markers, which can follow each other on one line.
const SECTION = /section_(start|end):\d+:([^\r\n[]+)(?:\[[^\]]*\])?\r?(?:\x1b\[0K)?/g;
const ESCAPE = /\x1b\[([0-9;?]*)([A-Za-z])/g;

type Style = Omit<Span, "text">;

function applySgr(style: Style, codes: number[]): Style {
  const s = { ...style };
  for (let i = 0; i < codes.length; i++) {
    const c = codes[i];
    if (c === 0) for (const k of Object.keys(s) as (keyof Style)[]) delete s[k];
    else if (c === 1) s.bold = true;
    else if (c === 2) s.dim = true;
    else if (c === 3) s.italic = true;
    else if (c === 4) s.underline = true;
    else if (c === 22) delete s.bold, delete s.dim;
    else if (c === 23) delete s.italic;
    else if (c === 24) delete s.underline;
    else if (c === 39) delete s.fg;
    else if (c >= 30 && c <= 37) s.fg = c - 30;
    else if (c >= 90 && c <= 97) s.fg = c - 82;
    else if (c === 38 || c === 48) {
      // 256 colours keep their first 16; true colour is dropped.
      if (codes[i + 1] === 5) {
        if (c === 38 && codes[i + 2] < 16) s.fg = codes[i + 2];
        i += 2;
      } else if (codes[i + 1] === 2) i += 4;
    }
  }
  return s;
}

/** The text of a line in styled runs. `style` carries over from the line before, as a terminal's does. */
export function spansOf(text: string, style: Style = {}): { spans: Span[]; style: Style } {
  const spans: Span[] = [];
  let at = 0;
  const push = (t: string) => t && spans.push({ text: t, ...style });
  for (const m of text.matchAll(ESCAPE)) {
    push(text.slice(at, m.index));
    at = m.index! + m[0].length;
    if (m[2] === "m") style = applySgr(style, m[1] ? m[1].split(";").map(Number) : [0]);
  }
  push(text.slice(at));
  return { spans, style };
}

/** A log's lines as entries: timestamps lifted out, the CI's markup read, progress redraws collapsed to what was left. */
export function parseLog(raw: string[]): LogEntry[] {
  const out: LogEntry[] = [];
  let group: { title: LogLine; lines: LogLine[] } | null = null;
  let style: Style = {};
  let n = 0;
  const add = (line: LogLine) => (group ? group.lines.push(line) : out.push(line));
  const open = (title: LogLine) => {
    if (group) out.push(group);
    group = { title, lines: [] };
  };
  const close = () => {
    if (group) out.push(group);
    group = null;
  };
  for (const source of raw) {
    let text = source;
    let time: string | null = null;
    const stamp = text.match(STAMP);
    if (stamp) {
      time = stamp[2];
      text = text.slice(stamp[0].length);
    }
    // GitLab: a section's end closes its group, a start opens one titled by the rest of the line.
    let starts = false;
    let marked = false;
    text = text.replace(SECTION, (_, kind: string) => {
      marked = true;
      if (kind === "end") close();
      else starts = true;
      return "";
    });
    const last = text.split("\r").filter(Boolean).at(-1) ?? "";
    let tone: Tone = null;
    let marker: string | null = null;
    const m = last.match(MARK);
    const body = m ? last.slice(m[0].length) : last.replace(/^##\[\w+\]/, "");
    if (m) marker = m[1];
    if (marker === "endgroup") {
      close();
      continue;
    }
    if (marker && marker !== "group") tone = marker as Tone;
    const parsed = spansOf(body, style);
    style = parsed.style;
    if (marked && !starts && !body.trim()) continue;
    const line: LogLine = { n: ++n, time, tone, spans: parsed.spans };
    if (marker === "group" || starts) open(line);
    else add(line);
  }
  close();
  return out;
}

// Where the Actions runner starts a step's output: a `run` or `uses` step's group, a post step, and the job's own cleanup.
const STEP_START = /^(##\[group\]Run |Post job cleanup\.|Cleaning up orphan processes)/;

/**
 * An Actions job's log cut into its steps, which start and end to the second only, so several can share one. A line
 * goes to the next step once it is stamped after the current step ended, or when it is where the runner starts a
 * step and the next step has started by then. An unstamped line goes with the one before.
 */
export function stepLines(raw: string[], steps: { start: string | null; end: string | null }[]): string[][] {
  const out: string[][] = steps.map(() => []);
  const sec = (iso: string | null | undefined) => (iso ? Math.floor(Date.parse(iso) / 1000) : Infinity);
  let i = 0;
  for (const line of raw) {
    const stamp = line.match(STAMP);
    if (stamp && steps.length) {
      const t = sec(stamp[1]);
      const text = line.slice(stamp[0].length);
      while (i + 1 < steps.length && sec(steps[i + 1].start) <= t && (t > sec(steps[i].end) || (STEP_START.test(text) && out[i].length > 0))) i++;
    }
    out[i]?.push(line);
  }
  return out;
}

/** The step a run view opens on: the first that failed, else the one running, else the longest. */
export function pickStep<S extends { state: CheckState; seconds: number | null }>(steps: S[]): S | undefined {
  return steps.find((s) => s.state === "failed") ?? steps.find((s) => s.state === "running") ?? steps.reduce<S | undefined>((a, s) => ((s.seconds ?? -1) > (a?.seconds ?? -1) ? s : a), undefined);
}
