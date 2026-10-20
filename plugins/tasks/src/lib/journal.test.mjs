import test from "node:test";
import assert from "node:assert/strict";
import { journalIssue, journalOf } from "./journal.ts";

test("a report keeps files with a path and real hashes", () => {
  const out = journalOf({ files: [{ path: "a.ts", what: "x\ny" }, { what: "no path" }, null, "str"], commits: ["abc1234", "xyz", 5, "ABCDEF1234567890"] });
  assert.deepEqual(out, { files: [{ path: "a.ts", what: "x y" }], commits: ["abc1234", "ABCDEF1234567890"] });
  assert.deepEqual(journalOf({ files: "no", commits: null }), { files: [], commits: [] });
});

test("the issue is the session's latest, and only while it is in progress or in review", () => {
  const issue = (key, status, updatedAt, ids = ["s"]) => ({ key, status, updatedAt, sessions: ids.map((id) => ({ id, seen: 1 })) });
  assert.equal(journalIssue([issue("A-1", "in_progress", 1), issue("A-2", "in_review", 5), issue("A-3", "in_progress", 9, ["other"])], "s").key, "A-2");
  assert.equal(journalIssue([issue("A-1", "in_progress", 1), issue("A-2", "done", 5)], "s"), undefined);
  assert.equal(journalIssue([issue("A-1", "todo", 1)], "s"), undefined);
  assert.equal(journalIssue([issue("A-1", "in_progress", 1)], "unknown"), undefined);
});

test("an issue the session touched this turn gets the report, closed or not started, and its child beats it", () => {
  const issue = (key, status, seen, parent) => ({ key, status, updatedAt: seen, parent, sessions: [{ id: "s", seen }] });
  // Closed as the turn's last step, while an older issue is still in progress.
  assert.equal(journalIssue([issue("A-1", "in_progress", 50), issue("A-2", "done", 120)], "s", 100).key, "A-2");
  assert.equal(journalIssue([issue("A-1", "todo", 120)], "s", 100).key, "A-1");
  // The epic was logged last, but the turn worked on its task.
  assert.equal(journalIssue([issue("E-1", "in_progress", 130), issue("A-3", "in_progress", 110, "E-1")], "s", 100).key, "A-3");
  assert.equal(journalIssue([issue("E-1", "in_progress", 130), issue("A-3", "in_progress", 90, "E-1")], "s", 100).key, "E-1", "a child from an earlier turn doesn't count");
  // Nothing touched this turn: the latest issue, only while it is live.
  assert.equal(journalIssue([issue("A-1", "in_progress", 50), issue("A-2", "done", 60)], "s", 100), undefined);
  assert.equal(journalIssue([issue("E-1", "in_progress", 60), issue("A-3", "in_review", 50, "E-1")], "s", 100).key, "A-3");
});

test("a report's start time is read when it is a real time", () => {
  assert.equal(journalOf({ started: "2026-10-20T14:02:11.000Z" }).started, Date.parse("2026-10-20T14:02:11.000Z"));
  assert.equal(journalOf({ started: "soon" }).started, undefined);
  assert.equal(journalOf({ started: 5 }).started, undefined);
});
