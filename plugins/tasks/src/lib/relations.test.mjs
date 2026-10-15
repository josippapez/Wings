import test from "node:test";
import assert from "node:assert/strict";
import { formatIssue, formatList } from "./format.ts";
import { applyOp } from "./ops.ts";
import { blocksOf, childrenOf, descendantsOf, parentChoices, progress } from "./relations.ts";
import { migrateTasks } from "./storage.ts";

const base = () => migrateTasks([], "web");
const make = (s, title, extra = {}) => applyOp(s, { kind: "create", title, ...extra }, "you", 1).store;
const up = (s, key, extra, by = "you") => applyOp(s, { kind: "update", key, ...extra }, by, 2);
const get = (s, key) => s.issues.find((i) => i.key === key);
/** WEB-1 epic, WEB-2 feature, WEB-3 task, WEB-4 bug. */
const sample = () => {
  let s = base();
  s = make(s, "Epic", { type: "epic" });
  s = make(s, "Feature", { type: "feature" });
  s = make(s, "Task");
  s = make(s, "Bug", { type: "bug" });
  return s;
};
const logs = (s, key) => get(s, key).log.map((e) => e.text);

test("type: changes are logged; a parent can't stop being an epic or feature while it has children", () => {
  let s = up(sample(), "WEB-3", { type: "bug" }).store;
  assert.equal(get(s, "WEB-3").type, "bug");
  assert.ok(logs(s, "WEB-3").includes("Type: Task → Bug"));
  s = up(s, "WEB-3", { parent: "WEB-1" }).store;
  assert.throws(() => up(s, "WEB-1", { type: "task" }), /has child issues/);
  assert.throws(() => up(s, "WEB-1", { type: "story" }), /type must be one of/);
});

test("parent: only an epic or feature, in this project, never itself, never a cycle", () => {
  let s = sample();
  assert.throws(() => up(s, "WEB-4", { parent: "WEB-3" }), /Only an epic or a feature/);
  assert.throws(() => up(s, "WEB-4", { parent: "WEB-99" }), /No issue WEB-99/);
  assert.throws(() => up(s, "WEB-1", { parent: "WEB-1" }), /itself/);
  s = up(s, "WEB-2", { parent: "WEB-1" }).store;
  assert.throws(() => up(s, "WEB-1", { parent: "WEB-2" }), /can't go under it/);
  s = up(s, "WEB-3", { parent: "web-2" }).store;
  assert.equal(get(s, "WEB-3").parent, "WEB-2");
  assert.deepEqual(childrenOf(s.issues, "WEB-2").map((i) => i.key), ["WEB-3"]);
  // deeper cycle: 1 > 2 > 3 can't make 3 a parent of 1, once 3 is a feature
  s = up(s, "WEB-3", { type: "feature" }).store;
  assert.throws(() => up(s, "WEB-1", { parent: "WEB-3" }), /can't go under it/);
});

test("parent: a link is logged on both issues, moving and clearing log on all three", () => {
  let s = up(sample(), "WEB-3", { parent: "WEB-1" }).store;
  assert.ok(logs(s, "WEB-3").includes("Parent: WEB-1"));
  assert.ok(logs(s, "WEB-1").includes("Child: WEB-3"));
  s = up(s, "WEB-3", { parent: "WEB-2" }).store;
  assert.ok(logs(s, "WEB-1").includes("Child removed: WEB-3"));
  assert.ok(logs(s, "WEB-2").includes("Child: WEB-3"));
  s = up(s, "WEB-3", { parent: "" }).store;
  assert.equal(get(s, "WEB-3").parent, undefined);
  assert.ok(logs(s, "WEB-3").includes("Parent removed (was WEB-2)"));
  assert.ok(logs(s, "WEB-2").includes("Child removed: WEB-3"));
  assert.equal(up(s, "WEB-3", { parent: "" }).changed, false);
});

test("parent can be set when creating, and parentChoices leaves out itself and what is below it", () => {
  let s = make(sample(), "New", { parent: "WEB-1", type: "feature" });
  assert.equal(get(s, "WEB-5").parent, "WEB-1");
  assert.ok(logs(s, "WEB-1").includes("Child: WEB-5"));
  assert.throws(() => make(sample(), "x", { parent: "WEB-3" }), /Only an epic/);
  assert.deepEqual(parentChoices(s.issues, get(s, "WEB-1")).map((i) => i.key), ["WEB-2"]);
  assert.deepEqual(parentChoices(s.issues, get(s, "WEB-3")).map((i) => i.key), ["WEB-1", "WEB-2", "WEB-5"]);
  assert.deepEqual(parentChoices(s.issues, get(s, "WEB-2")).map((i) => i.key), ["WEB-1", "WEB-5"]);
  // WEB-5 sits under WEB-1, so WEB-1 can no longer go under it
  s = up(s, "WEB-1", { parent: "WEB-2" }).store;
  assert.deepEqual(parentChoices(s.issues, get(s, "WEB-2")).map((i) => i.key), []);
});

test("related is symmetric, deduped, capped at 20, logged on both and removable from either side", () => {
  let s = up(sample(), "WEB-3", { relatedAdd: ["web-4", "WEB-4"] }).store;
  assert.deepEqual(get(s, "WEB-3").related, ["WEB-4"]);
  assert.deepEqual(get(s, "WEB-4").related, ["WEB-3"]);
  assert.ok(logs(s, "WEB-3").includes("Linked to WEB-4 as related"));
  assert.ok(logs(s, "WEB-4").includes("Linked to WEB-3 as related"));
  assert.equal(up(s, "WEB-4", { relatedAdd: ["WEB-3"] }).changed, false);
  assert.throws(() => up(s, "WEB-3", { relatedAdd: ["WEB-3"] }), /itself/);
  s = up(s, "WEB-4", { relatedRemove: ["WEB-3"] }).store;
  assert.equal(get(s, "WEB-3").related, undefined);
  assert.equal(get(s, "WEB-4").related, undefined);
  assert.ok(logs(s, "WEB-3").includes("Unlinked from WEB-4 (related)"));
  // the cap
  let big = base();
  for (let i = 0; i < 23; i++) big = make(big, `t${i}`);
  for (let i = 2; i <= 21; i++) big = up(big, "WEB-1", { relatedAdd: [`WEB-${i}`] }).store;
  assert.equal(get(big, "WEB-1").related.length, 20);
  assert.throws(() => up(big, "WEB-1", { relatedAdd: ["WEB-22"] }), /already has 20 related/);
  assert.throws(() => up(big, "WEB-22", { relatedAdd: ["WEB-1"] }), /WEB-1 already has 20/);
});

test("blocked by has an inverse blocks, logged on both, and refuses a loop", () => {
  let s = up(sample(), "WEB-4", { blockedByAdd: ["WEB-3"] }).store;
  assert.deepEqual(get(s, "WEB-4").blockedBy, ["WEB-3"]);
  assert.deepEqual(blocksOf(s.issues, "WEB-3").map((i) => i.key), ["WEB-4"]);
  assert.ok(logs(s, "WEB-4").includes("Blocked by WEB-3"));
  assert.ok(logs(s, "WEB-3").includes("Blocks WEB-4"));
  assert.throws(() => up(s, "WEB-3", { blockedByAdd: ["WEB-4"] }), /block each other/);
  s = up(s, "WEB-4", { blockedByAdd: ["WEB-2"] }).store;
  s = up(s, "WEB-2", { blockedByAdd: ["WEB-1"] }).store;
  assert.throws(() => up(s, "WEB-1", { blockedByAdd: ["WEB-4"] }), /block each other/);
  s = up(s, "WEB-4", { blockedByRemove: ["WEB-3"] }).store;
  assert.deepEqual(get(s, "WEB-4").blockedBy, ["WEB-2"]);
  assert.ok(logs(s, "WEB-3").includes("No longer blocks WEB-4"));
});

test("a failed relation change leaves the store and the other issues as they were", () => {
  const s = up(sample(), "WEB-3", { relatedAdd: ["WEB-4"] }).store;
  const before = JSON.stringify(s);
  assert.throws(() => up(s, "WEB-3", { relatedAdd: ["WEB-1"], parent: "WEB-4" }));
  assert.equal(JSON.stringify(s), before);
});

test("deleting an issue removes it from the others' relations and says so in their logs", () => {
  let s = sample();
  s = up(s, "WEB-3", { parent: "WEB-1", relatedAdd: ["WEB-2"], blockedByAdd: ["WEB-4"] }).store;
  s = up(s, "WEB-2", { parent: "WEB-1" }).store;
  const gone = applyOp(s, { kind: "remove", key: "WEB-1" }, "you", 9).store;
  assert.equal(get(gone, "WEB-3").parent, undefined);
  assert.equal(get(gone, "WEB-2").parent, undefined);
  assert.ok(logs(gone, "WEB-3").includes("Parent WEB-1 was deleted"));
  const gone2 = applyOp(s, { kind: "remove", key: "WEB-2" }, "you", 9).store;
  assert.equal(get(gone2, "WEB-3").related, undefined);
  assert.ok(logs(gone2, "WEB-3").includes("WEB-2 was deleted: unlinked (related)"));
  const gone3 = applyOp(s, { kind: "remove", key: "WEB-4" }, "you", 9).store;
  assert.equal(get(gone3, "WEB-3").blockedBy, undefined);
  assert.ok(logs(gone3, "WEB-3").includes("WEB-4 was deleted: no longer blocking"));
  assert.equal(gone3.next, s.next);
});

test("deleting with children takes the whole subtree; the rest let go of it in their logs", () => {
  let s = sample();
  s = up(s, "WEB-2", { parent: "WEB-1" }).store;
  s = up(s, "WEB-3", { parent: "WEB-2" }).store;
  s = up(s, "WEB-4", { relatedAdd: ["WEB-3"], blockedByAdd: ["WEB-2"] }).store;
  assert.deepEqual(descendantsOf(s.issues, "WEB-1").map((i) => i.key), ["WEB-2", "WEB-3"]);
  const gone = applyOp(s, { kind: "remove", key: "WEB-1", children: true }, "you", 9).store;
  assert.deepEqual(gone.issues.map((i) => i.key), ["WEB-4"]);
  assert.equal(get(gone, "WEB-4").related, undefined);
  assert.equal(get(gone, "WEB-4").blockedBy, undefined);
  assert.ok(logs(gone, "WEB-4").includes("WEB-3 was deleted: unlinked (related)"));
  assert.ok(logs(gone, "WEB-4").includes("WEB-2 was deleted: no longer blocking"));
  assert.equal(gone.next, s.next);
  const kept = applyOp(s, { kind: "remove", key: "WEB-1" }, "you", 9).store;
  assert.deepEqual(kept.issues.map((i) => i.key), ["WEB-2", "WEB-3", "WEB-4"]);
  assert.equal(get(kept, "WEB-2").parent, undefined);
  assert.equal(get(kept, "WEB-3").parent, "WEB-2");
});

test("progress counts done of all children, canceled ones left out", () => {
  const mk = (status) => ({ status });
  assert.deepEqual(progress([mk("done"), mk("todo"), mk("canceled"), mk("done")]), { done: 2, total: 3 });
  assert.deepEqual(progress([]), { done: 0, total: 0 });
});

test("get shows the relations and list the type and parent", () => {
  let s = up(sample(), "WEB-3", { parent: "WEB-1", relatedAdd: ["WEB-4"] }).store;
  s = up(s, "WEB-4", { blockedByAdd: ["WEB-3"] }).store;
  const epic = formatIssue(get(s, "WEB-1"), s.issues);
  assert.match(epic, /\(epic\) Epic/);
  assert.match(epic, /Child issues \(0 of 1 done\):\n {2}WEB-3 \[todo\] Task/);
  const task = formatIssue(get(s, "WEB-3"), s.issues);
  assert.match(task, /Parent: WEB-1 \[todo\] \(epic\) Epic/);
  assert.match(task, /Related:\n {2}WEB-4 \[todo\] Bug/);
  assert.match(task, /Blocks:\n {2}WEB-4/);
  assert.doesNotMatch(task, /Child issues/);
  assert.match(formatList(s.issues, undefined, "web", "bug"), /WEB-4 \[todo\] \(bug\) Bug/);
  assert.doesNotMatch(formatList(s.issues, undefined, "web", "bug"), /WEB-3/);
  assert.match(formatList(s.issues, undefined, "web"), /WEB-3 \[todo\] \(task\) Task in WEB-1/);
});
