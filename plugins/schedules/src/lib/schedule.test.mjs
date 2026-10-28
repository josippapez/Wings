import test from "node:test";
import assert from "node:assert/strict";
import { claim, cleanPrompt, describeNext, dueAt, nextRun, onStart } from "./schedule.ts";

// Summer time starts on Sunday 2026-03-29 here, so a day is 23 hours long once.
process.env.TZ = "Europe/Zagreb";

const at = (y, m, d, h = 0, min = 0) => new Date(y, m - 1, d, h, min).getTime();
const make = (when, anchor, extra = {}) => ({ id: "a", name: "", projectId: "p", agent: "claude", prompt: "Review", when, enabled: true, anchor, ...extra });
const daily = (time, days) => ({ kind: "daily", time, days });
const ALL = [0, 1, 2, 3, 4, 5, 6];

test("every N hours counts from the anchor, across midnight", () => {
  const s = make({ kind: "every", hours: 6 }, at(2026, 5, 4, 21, 15));
  assert.equal(nextRun(s), at(2026, 5, 5, 3, 15));
  assert.equal(dueAt(s, at(2026, 5, 5, 3, 14)), null);
  assert.equal(dueAt(s, at(2026, 5, 5, 3, 15)), at(2026, 5, 5, 3, 15));
});

test("daily runs at the time on the chosen days, and skips the others", () => {
  // 2026-05-04 is a Monday. Only Mon and Thu: after Monday's 09:00 comes Thursday's.
  const s = make(daily("09:00", [1, 4]), at(2026, 5, 4, 9, 0));
  assert.equal(nextRun(s), at(2026, 5, 7, 9, 0));
  // Before today's time it runs today; after it, the next day picked.
  assert.equal(nextRun({ ...s, anchor: at(2026, 5, 4, 8, 59) }), at(2026, 5, 4, 9, 0));
  assert.equal(nextRun({ ...s, anchor: at(2026, 5, 7, 9, 1) }), at(2026, 5, 11, 9, 0));
  // Late evening to the next morning, over midnight.
  assert.equal(nextRun(make(daily("00:30", ALL), at(2026, 5, 4, 23, 50))), at(2026, 5, 5, 0, 30));
  assert.equal(nextRun(make(daily("09:00", []), 0)), null);
});

test("daily follows the clock on the wall through the change to summer time", () => {
  const s = make(daily("09:00", ALL), at(2026, 3, 28, 9, 0));
  assert.equal(nextRun(s), at(2026, 3, 29, 9, 0));
  assert.equal(nextRun(s) - s.anchor, 23 * 3_600_000);
  // 02:30 doesn't exist on the 29th: it runs once that day, not twice and not never.
  const skipped = nextRun(make(daily("02:30", ALL), at(2026, 3, 28, 2, 30)));
  assert.equal(new Date(skipped).getDate(), 29);
  assert.equal(nextRun(make(daily("02:30", ALL), skipped)), at(2026, 3, 30, 2, 30));
});

test("a paused schedule is never due", () => {
  const s = make({ kind: "every", hours: 1 }, at(2026, 5, 4, 8), { enabled: false });
  assert.equal(dueAt(s, at(2026, 5, 4, 20)), null);
});

test("a due time starts once: claiming it moves the schedule past it", () => {
  const s = make({ kind: "every", hours: 2 }, at(2026, 5, 4, 8));
  const due = dueAt(s, at(2026, 5, 4, 10, 0) + 20_000);
  assert.equal(due, at(2026, 5, 4, 10));
  const claimed = claim(s, due, at(2026, 5, 4, 10, 0) + 20_000);
  // The next tick, and a second frame reading the claimed schedule, find nothing due.
  assert.equal(dueAt(claimed, at(2026, 5, 4, 10, 0) + 50_000), null);
  // On time, the next run keeps the schedule's rhythm.
  assert.equal(nextRun(claimed), at(2026, 5, 4, 12));
  // Started late (the Mac slept), the next run counts from now instead of firing again at once.
  const late = claim(s, due, at(2026, 5, 4, 15));
  assert.equal(dueAt(late, at(2026, 5, 4, 15, 1)), null);
  assert.equal(nextRun(late), at(2026, 5, 4, 17));
});

test("a schedule that came due while Wings was closed isn't made up", () => {
  const s = make(daily("09:00", ALL), at(2026, 5, 4, 9, 0));
  const now = at(2026, 5, 6, 12);
  assert.notEqual(dueAt(s, now), null);
  const started = onStart(s, now);
  assert.equal(dueAt(started, now), null);
  assert.equal(nextRun(started), at(2026, 5, 7, 9, 0));
  // One that isn't due yet keeps its time.
  const waiting = make({ kind: "every", hours: 12 }, at(2026, 5, 6, 8));
  assert.equal(onStart(waiting, now), waiting);
});

test("a prompt is one line that can't read as a flag", () => {
  assert.equal(cleanPrompt("  Fix the\n\n  build\tplease "), "Fix the build please");
  assert.throws(() => cleanPrompt("--resume"), /dash/);
  assert.throws(() => cleanPrompt(" \n "), /Write what/);
});

test("next run reads as a wait, then as a day and time", () => {
  const now = at(2026, 5, 4, 10, 0);
  assert.equal(describeNext(now + 25 * 60_000, now), "in 25 min");
  assert.equal(describeNext(now + 3 * 3_600_000, now), "in 3 h");
  assert.equal(describeNext(at(2026, 5, 4, 22, 0), now), "Today 22:00");
  assert.equal(describeNext(at(2026, 5, 5, 9, 0), now), "Tomorrow 09:00");
  assert.equal(describeNext(at(2026, 5, 11, 9, 0), now), "Mon 09:00");
});
