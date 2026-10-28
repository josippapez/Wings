// What a schedule is and when it runs next. All of it is plain date math in local time, so a tick costs nothing and a
// daily time follows the clock on the wall, through a change to or from summer time.

export type Agent = "claude" | "opencode";

export type When =
  | { kind: "every"; hours: number }
  /** `time` is "HH:MM" and `days` are `Date.getDay()` values, 0 for Sunday. */
  | { kind: "daily"; time: string; days: number[] };

export interface Run {
  at: number;
  /** The pane the agent started in, when it did. */
  paneId?: string;
  ok: boolean;
  error?: string;
}

export interface Schedule {
  id: string;
  /** Left empty, the list shows the start of the prompt. */
  name: string;
  projectId: string;
  agent: Agent;
  prompt: string;
  when: When;
  enabled: boolean;
  /** Where the next run is counted from: when it was saved, resumed or last started, or when Wings started and found it overdue. */
  anchor: number;
  lastRun?: Run;
}

export type Draft = Omit<Schedule, "id" | "anchor" | "enabled" | "lastRun">;

/** `command` is what `openPane` starts, with the prompt as its one argument, and what the manifest's `panes` allows. */
export const AGENTS: Record<Agent, { label: string; command: string }> = {
  claude: { label: "Claude Code", command: "claude" },
  // `opencode --help`: --prompt is the first message.
  opencode: { label: "OpenCode", command: "opencode --prompt" },
};

export const EVERY_HOURS = [1, 2, 3, 4, 6, 8, 12, 24];
export const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/** `Date.getDay()` for each of `DAYS`. */
export const DAY_NUMBERS = [1, 2, 3, 4, 5, 6, 0];
export const MAX_PROMPT_BYTES = 4000;

const HOUR = 3_600_000;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;
/** A run that starts later than this after its time was held up (the Mac slept), so the next one counts from now. */
const LATE_MS = 120_000;

/** When the schedule runs next, in ms, or null when no day is picked. */
export function nextRun(s: Schedule): number | null {
  if (s.when.kind === "every") return s.anchor + s.when.hours * HOUR;
  const { time, days } = s.when;
  if (!days.length) return null;
  const [hh, mm] = time.split(":").map(Number);
  const from = new Date(s.anchor);
  for (let i = 0; i <= 7; i++) {
    const at = new Date(from.getFullYear(), from.getMonth(), from.getDate() + i, hh, mm).getTime();
    if (at > s.anchor && days.includes(new Date(at).getDay())) return at;
  }
  return null;
}

/** The time a run is due, or null while the schedule is paused or not yet there. */
export function dueAt(s: Schedule, now: number): number | null {
  if (!s.enabled) return null;
  const at = nextRun(s);
  return at !== null && at <= now ? at : null;
}

/** The schedule once the run due at `due` has been claimed: it can't be due at `due` again. */
export function claim(s: Schedule, due: number, now: number): Schedule {
  return { ...s, anchor: now - due < LATE_MS ? due : now };
}

/** On start, a schedule that came due while Wings was closed counts from now: nothing is made up. */
export function onStart(s: Schedule, now: number): Schedule {
  return dueAt(s, now) === null ? s : { ...s, anchor: now };
}

export const nameOf = (s: Pick<Schedule, "name" | "prompt">): string => {
  const name = s.name.trim();
  if (name) return name;
  return s.prompt.length > 48 ? `${s.prompt.slice(0, 47).trimEnd()}…` : s.prompt;
};

/** The prompt as `openPane` takes it: one line, and nothing a shell or the agent's own flags could read. Throws what the form shows. */
export function cleanPrompt(raw: string): string {
  const prompt = raw.replace(/\s*[\x00-\x1f\x7f]+\s*/g, " ").trim();
  if (!prompt) throw new Error("Write what the agent should do.");
  if (prompt.startsWith("-")) throw new Error("The prompt can't start with a dash. Begin it with a word.");
  if (new TextEncoder().encode(prompt).length > MAX_PROMPT_BYTES) throw new Error("The prompt is too long. Keep it under 4 KB.");
  return prompt;
}

/** A draft that is safe to save, or the reason it isn't. */
export function cleanDraft(d: Draft): Draft {
  if (!Object.hasOwn(AGENTS, d.agent)) throw new Error("Pick Claude Code or OpenCode.");
  if (!d.projectId) throw new Error("Pick a project.");
  let when: When;
  if (d.when.kind === "every") {
    if (!EVERY_HOURS.includes(d.when.hours)) throw new Error("Pick how many hours.");
    when = { kind: "every", hours: d.when.hours };
  } else {
    if (!TIME.test(d.when.time)) throw new Error("Pick a time of day.");
    const days = [...new Set(d.when.days)].filter((n) => DAY_NUMBERS.includes(n)).sort((a, b) => DAY_NUMBERS.indexOf(a) - DAY_NUMBERS.indexOf(b));
    if (!days.length) throw new Error("Pick at least one day.");
    when = { kind: "daily", time: d.when.time, days };
  }
  return { name: d.name.trim().slice(0, 80), projectId: d.projectId, agent: d.agent, prompt: cleanPrompt(d.prompt), when };
}

export function describeWhen(when: When): string {
  if (when.kind === "every") return when.hours === 1 ? "Every hour" : `Every ${when.hours} hours`;
  const picked = DAY_NUMBERS.filter((n) => when.days.includes(n));
  let days = picked.map((n) => DAYS[DAY_NUMBERS.indexOf(n)]).join(", ");
  if (picked.length === 7) days = "Every day";
  else if (picked.length === 5 && picked.every((n) => n >= 1 && n <= 5)) days = "Weekdays";
  return `${days} at ${when.time}`;
}

const pad = (n: number) => String(n).padStart(2, "0");
const clock = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** "in 25 min", "in 3 h", "Today 18:00", "Tomorrow 09:00", "Mon 09:00". */
export function describeNext(at: number, now: number): string {
  const wait = at - now;
  if (wait < 60_000) return "in under a minute";
  if (wait < HOUR) return `in ${Math.round(wait / 60_000)} min`;
  if (wait < 12 * HOUR) return `in ${Math.round(wait / HOUR)} h`;
  const d = new Date(at);
  const today = new Date(now);
  const days = Math.round((new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() - new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()) / (24 * HOUR));
  if (days === 0) return `Today ${clock(d)}`;
  if (days === 1) return `Tomorrow ${clock(d)}`;
  return `${DAYS[DAY_NUMBERS.indexOf(d.getDay())]} ${clock(d)}`;
}
