// What the tools need to add or change a timed schedule: its timing from the tool's fields, and what the user is asked to
// allow. A timed schedule starts an agent by itself, so an agent can't add one, or change what or when it runs, unless the
// user allows it. No `wings`, so node tests can drive it.

import { EVERY_HOURS, type Draft, type When } from "./schedule";

/** The tools' day names, at their `Date.getDay()` index. */
export const DAY_NAMES = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"] as const;
export type DayName = (typeof DAY_NAMES)[number];

export interface WhenInput {
  every_hours?: number;
  time?: string;
  days?: DayName[];
}

/**
 * The timing the tool's fields give, over `current` for a change. `time` without `days` keeps the current days, or is
 * every day for a new one. Undefined when no field is given. Throws what to fix.
 */
export function whenFrom({ every_hours, time, days }: WhenInput, current?: When): When | undefined {
  if (every_hours !== undefined && (time !== undefined || days !== undefined)) throw new Error("Give every_hours, or time with days, not both.");
  if (every_hours !== undefined) {
    if (!EVERY_HOURS.includes(every_hours)) throw new Error(`every_hours is one of ${EVERY_HOURS.join(", ")}.`);
    return { kind: "every", hours: every_hours };
  }
  const daily = current?.kind === "daily" ? current : undefined;
  if (time === undefined && days === undefined) return undefined;
  if (time === undefined && !daily) throw new Error("days goes with a time of day: give time too.");
  return {
    kind: "daily",
    time: time ?? daily!.time,
    days: days ? days.map((d) => DAY_NAMES.indexOf(d)) : (daily?.days ?? [0, 1, 2, 3, 4, 5, 6]),
  };
}

/** What the user is asked to allow. */
export interface Approval {
  id: string;
  /** The agent session that asks, or its pane's project, or null when neither is known. */
  asker: string | null;
  projectName: string;
  /** The schedule as it is, for a change; null for a new one. */
  current: Draft | null;
  proposed: Draft;
}

/** Whether saving `next` over `prev` changes what runs, where or when. A new name alone doesn't need the user. */
export function needsApproval(prev: Draft, next: Draft): boolean {
  return prev.projectId !== next.projectId || prev.agent !== next.agent || prev.prompt !== next.prompt || JSON.stringify(prev.when) !== JSON.stringify(next.when);
}
