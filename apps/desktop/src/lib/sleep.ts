import type { Agent } from "./api.ts";
import { isSessionIdOf } from "./resume.ts";

/** Sleeping ends an idle agent that nobody looks at, so its memory comes back: about 400 MB for an idle Claude Code (measured). */
export type SleepSettings = { enabled: boolean; minutes: number };
export const DEFAULT_SLEEP: SleepSettings = { enabled: true, minutes: 15 };
export const SLEEP_MINUTES = [5, 10, 15, 30, 60];

/** How long a resumed agent may take to show up before the row says it couldn't resume. */
export const RESUME_TIMEOUT_MS = 20_000;
/** How long an agent gets to exit after `/exit` before Wings stops counting the pane as asleep. */
export const EXIT_GRACE_MS = 15_000;

/** What decides whether an agent may be put to sleep. Anything that can't be told counts against it. */
export type SleepCheck = {
  agent: Agent | undefined;
  /** The pane's tab is on screen. */
  visible: boolean;
  /** Typed text is waiting in the prompt (see `PaneInput`). */
  draft: boolean;
  /** Already asleep, or being woken. */
  busy: boolean;
  /** The agent process has a child doing work, like a background command or dev server (`agent_busy`). */
  background: boolean;
};

/** Idle (a finished turn you haven't seen is "done", and needs you), out of sight, with a session to come back to. */
export function canSleep({ agent, visible, draft, busy, background }: SleepCheck) {
  if (!agent || visible || draft || busy || background) return false;
  // `direct`: the pane's shell goes back to its prompt when the agent exits, so nothing else runs in its place.
  return agent.state === "idle" && agent.direct && !!agent.sessionId && isSessionIdOf(agent.kind, agent.sessionId);
}

/**
 * Tracks how long each pane has been sleep-eligible. `eligible` is every pane eligible right now: one that stops
 * being eligible loses its clock, one that starts gets `now`. Returns the panes whose clock ran out, and when the
 * next one will, so a single timer does the waiting and nothing polls.
 */
export function planSleep(since: ReadonlyMap<string, number>, eligible: readonly string[], now: number, minutes: number) {
  const next = new Map(eligible.map((key) => [key, since.get(key) ?? now]));
  const limit = minutes * 60_000;
  const due = [...next].filter(([, from]) => now - from >= limit).map(([key]) => key);
  const waiting = [...next].filter(([, from]) => now - from < limit).map(([, from]) => from + limit);
  return { since: next, due, nextAt: waiting.length ? Math.min(...waiting) : null };
}

/** What a pane's row shows when no live agent is in it. */
export type Phase = "restored" | "sleeping" | "starting" | "failed";

export type ResumeMeta = {
  paneId: string | null;
  sessionId: string | null;
  /** Ended by Wings to free memory, waiting to be opened again. */
  asleep: boolean;
  /** When Wings started bringing the session back; null when it isn't. */
  resumeAt: number | null;
  /** The agent didn't show up within `RESUME_TIMEOUT_MS`. */
  resumeFailed: boolean;
};

export const noResume = { asleep: false, resumeAt: null, resumeFailed: false } as const;

/**
 * The row for a pane with a session to come back to, or null when it has none or a live agent is there (that
 * agent has its own row). "starting" holds the place from the moment you ask until the agent is seen.
 */
export function phaseOf(meta: ResumeMeta, agent: Agent | undefined): Phase | null {
  if (agent || !meta.sessionId) return null;
  if (meta.resumeAt !== null) return meta.resumeFailed ? "failed" : "starting";
  if (meta.asleep) return "sleeping";
  return meta.paneId ? null : "restored";
}

/** The agent showed up in the pane: the resume is over, whatever state it was in. */
export const settled = (meta: ResumeMeta, agent: Agent | undefined) => !!agent && (meta.asleep || meta.resumeAt !== null || meta.resumeFailed);

/** When the earliest resume still waiting runs out, or null when none waits. */
export function nextResumeDeadline(metas: readonly ResumeMeta[]) {
  const times = metas.filter((m) => m.resumeAt !== null && !m.resumeFailed).map((m) => m.resumeAt! + RESUME_TIMEOUT_MS);
  return times.length ? Math.min(...times) : null;
}

/** Whether a resume that began at `resumeAt` has run out by `now`. */
export const resumeExpired = (resumeAt: number, now: number) => now - resumeAt >= RESUME_TIMEOUT_MS;
