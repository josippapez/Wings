import assert from "node:assert/strict";
import { test } from "node:test";

import { session } from "./session.ts";
import { limitsFor, shown, snapshot, state } from "./view.ts";

const sessions = [{ id: "a" }, { id: "b" }, { id: "c" }];

test("the sidebar shows the session of the selected pane, and one picked from the list until another pane is selected", () => {
  // Nothing selected yet: the first.
  assert.equal(shown(sessions, null, null).id, "a");
  assert.equal(shown(sessions, "b", null).id, "b");
  // Picked while b's pane was selected: it shows, until the selection moves on.
  const picked = { id: "c", over: "b" };
  assert.equal(shown(sessions, "b", picked).id, "c");
  assert.equal(shown(sessions, "a", picked).id, "a");
  // The followed session's pane closed.
  assert.equal(shown(sessions, "gone", null).id, "a");
  assert.equal(shown([], "a", null), undefined);
});

test("the shown session's account's limits come first, and another account's never pass for its own", () => {
  const limit = (account) => ({ account, fiveHour: null, sevenDay: null });
  const both = [limit(null), limit("work")];
  assert.deepEqual(limitsFor(both, "work"), { limits: [limit("work"), limit(null)], missing: false });
  assert.deepEqual(limitsFor(both, null), { limits: both, missing: false });
  // Claude Code hasn't said which account the session runs as: the limits as they are.
  assert.deepEqual(limitsFor(both, undefined), { limits: both, missing: false });
  assert.deepEqual(limitsFor([limit(null)], "work"), { limits: [limit(null)], missing: true });
  assert.deepEqual(limitsFor([], "work"), { limits: [], missing: false });
});

test("the snapshot says which session's pane is selected and the account each runs as", () => {
  const [one, two] = [session("one"), session("two")];
  Object.assign(one, { project: "a" });
  Object.assign(two, { project: "b", focused: true });
  state.reported = { rateLimits: [], sessions: { two: { account: "work", at: 0 } } };
  const views = snapshot([one, two]).sessions;
  assert.deepEqual(views.map((s) => [s.id, s.focused, s.account]), [["one", false, undefined], ["two", true, "work"]]);
  state.reported = null;
});
