import test from "node:test";
import assert from "node:assert/strict";
import { needsApproval, whenFrom } from "./approval.ts";

const daily = { kind: "daily", time: "09:00", days: [1, 2, 3, 4, 5] };

test("a time with no days runs every day, or keeps a daily schedule's days", () => {
  assert.deepEqual(whenFrom({ time: "09:00" }), { kind: "daily", time: "09:00", days: [0, 1, 2, 3, 4, 5, 6] });
  assert.deepEqual(whenFrom({ time: "18:30" }, daily), { kind: "daily", time: "18:30", days: [1, 2, 3, 4, 5] });
  assert.deepEqual(whenFrom({ days: ["sat", "sun"] }, daily), { kind: "daily", time: "09:00", days: [6, 0] });
});

test("every_hours takes only the hours the page offers, and not with a time", () => {
  assert.deepEqual(whenFrom({ every_hours: 4 }, daily), { kind: "every", hours: 4 });
  assert.throws(() => whenFrom({ every_hours: 5 }), /one of 1, 2, 3, 4, 6, 8, 12, 24/);
  assert.throws(() => whenFrom({ every_hours: 4, time: "09:00" }), /not both/);
  assert.throws(() => whenFrom({ days: ["mon"] }, { kind: "every", hours: 4 }), /give time too/);
  assert.equal(whenFrom({}, daily), undefined);
});

test("only what runs, where or when needs the user's approval, not a new name", () => {
  const draft = { name: "", projectId: "p1", agent: "claude", prompt: "Check the board.", when: daily };
  assert.equal(needsApproval(draft, { ...draft, name: "Board" }), false);
  assert.equal(needsApproval(draft, { ...draft, prompt: "Delete the board." }), true);
  assert.equal(needsApproval(draft, { ...draft, agent: "opencode" }), true);
  assert.equal(needsApproval(draft, { ...draft, when: { ...daily, time: "03:00" } }), true);
});
