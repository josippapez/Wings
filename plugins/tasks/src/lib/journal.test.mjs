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
