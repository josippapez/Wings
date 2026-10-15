import assert from "node:assert/strict";
import test from "node:test";
import { between, durationText, groupChecks, hasLog, logTail, withUnresolved } from "./checklist.ts";

const item = (name, state, extra = {}) => ({ id: name, name, state, required: null, kind: null, at: null, seconds: null, url: null, note: null, ...extra });

test("groups run failed, running and queued, passed, then optional and not run, and skip empty ones", () => {
  const groups = groupChecks([
    item("lint", "passed"),
    item("deploy preview", "skipped"),
    item("e2e", "queued"),
    item("unit", "failed"),
    item("build", "running"),
    item("coverage", "passed", { required: false }),
  ]);
  assert.deepEqual(groups.map((g) => [g.id, g.items.map((c) => c.name)]), [
    ["failed", ["unit"]],
    ["active", ["build", "e2e"]],
    ["passed", ["lint"]],
    ["other", ["coverage", "deploy preview"]],
  ]);
  assert.deepEqual(groupChecks([item("a", "passed")]).map((g) => g.id), ["passed"]);
  assert.deepEqual(groupChecks([]), []);
});

test("an optional failure still shows under Failed, and required checks come before optional ones", () => {
  const groups = groupChecks([item("b", "failed", { required: false }), item("a", "failed", { required: true }), item("c", "failed")]);
  assert.deepEqual(groups[0].items.map((c) => c.name), ["a", "b", "c"]);
});

test("durations read as seconds, minutes or hours, and bad times give nothing", () => {
  assert.equal(durationText(45), "45s");
  assert.equal(durationText(134.4), "2m 14s");
  assert.equal(durationText(3900), "1h 05m");
  assert.equal(durationText(null), null);
  assert.equal(durationText(-1), null);
  assert.equal(between("2026-01-01T00:00:00Z", "2026-01-01T00:02:14Z"), 134);
  assert.equal(between("0001-01-01T00:00:00Z", "2026-01-01T00:02:14Z"), null);
  assert.equal(between("2026-01-02T00:00:00Z", "2026-01-01T00:00:00Z"), null);
});

test("a log can be read where the failure names a job or a build", () => {
  assert.equal(hasLog({ id: 1, name: "unit", job: "77" }), true);
  assert.equal(hasLog({ id: 2, name: "ci", build: 42 }), true);
  assert.equal(hasLog({ id: 3, name: "deploy" }), false);
  assert.equal(hasLog(undefined), false);
});

test("a comment requirement counts the open threads once they are known", () => {
  const c = item("Comment requirements", "failed", { needs: "unresolved", note: "Comment threads are unresolved" });
  assert.equal(withUnresolved([c], null)[0].note, c.note);
  assert.equal(withUnresolved([c], 3)[0].note, "3 comment threads unresolved");
  assert.equal(withUnresolved([c], 1)[0].note, "1 comment thread unresolved");
  assert.equal(withUnresolved([c], 0)[0].note, "All comment threads resolved");
  assert.equal(withUnresolved([item("x", "passed", { note: "n" })], 5)[0].note, "n");
});

test("a log shows its last lines and says how many came before", () => {
  const log = Array.from({ length: 100 }, (_, i) => `line ${i}`).join("\n");
  const { text, hidden } = logTail(log);
  assert.equal(hidden, 60);
  assert.equal(text.split("\n").length, 40);
  assert.equal(text.split("\n").at(-1), "line 99");
  assert.deepEqual(logTail("a\nb"), { text: "a\nb", hidden: 0 });
});
