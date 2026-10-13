import test from "node:test";
import assert from "node:assert/strict";
import { applyOp, boardColumns, cleanBlockState, openTasks, LIMITS } from "./tasks.ts";

const add = (tasks, title, extra = {}, now = 1) => applyOp(tasks, { kind: "add", title, ...extra }, now).tasks;

test("add trims, defaults to todo and keeps the branch", () => {
  const [t] = add([], "  Fix it ", { branch: "feat/x" });
  assert.equal(t.title, "Fix it");
  assert.equal(t.status, "todo");
  assert.deepEqual(t.links, { branch: "feat/x" });
});

test("add rejects an empty or too long title", () => {
  assert.throws(() => add([], "   "), /required/);
  assert.throws(() => add([], "x".repeat(LIMITS.title + 1)), /limited/);
});

test("update changes only what is passed, and clears notes with an empty string", () => {
  let tasks = add([], "A", { notes: "n" });
  tasks = applyOp(tasks, { kind: "update", id: tasks[0].id, status: "doing" }, 5).tasks;
  assert.equal(tasks[0].notes, "n");
  assert.equal(tasks[0].status, "doing");
  assert.equal(tasks[0].updatedAt, 5);
  tasks = applyOp(tasks, { kind: "update", id: tasks[0].id, notes: "" }).tasks;
  assert.equal(tasks[0].notes, undefined);
});

test("unknown id and bad status are errors", () => {
  assert.throws(() => applyOp([], { kind: "update", id: "nope", status: "done" }), /No task/);
  const tasks = add([], "A");
  assert.throws(() => applyOp(tasks, { kind: "update", id: tasks[0].id, status: "later" }), /status/);
});

test("a full project drops its oldest done task, or refuses when none is done", () => {
  let tasks = [];
  for (let i = 0; i < LIMITS.tasks; i++) tasks = add(tasks, `t${i}`, {}, i);
  assert.throws(() => add(tasks, "more"), /none is done/);
  tasks = applyOp(tasks, { kind: "update", id: tasks[3].id, status: "done" }, 500).tasks;
  const next = add(tasks, "more");
  assert.equal(next.length, LIMITS.tasks);
  assert.ok(!next.some((t) => t.title === "t3"));
});

test("openTasks lists doing first, then todo by age, and no done", () => {
  let tasks = add(add(add([], "a", {}, 1), "b", {}, 2), "c", {}, 3);
  tasks = applyOp(tasks, { kind: "update", id: tasks[2].id, status: "doing" }).tasks;
  tasks = applyOp(tasks, { kind: "update", id: tasks[0].id, status: "done" }).tasks;
  assert.deepEqual(openTasks(tasks).map((t) => t.title), ["c", "b"]);
});

test("boardColumns: three columns, oldest first, done newest first; hideDone drops done", () => {
  const t = (id, status, createdAt, updatedAt = createdAt) => ({ id, title: id, status, createdAt, updatedAt });
  const tasks = [t("b", "todo", 2), t("a", "todo", 1), t("d1", "done", 1, 10), t("d2", "done", 2, 20), t("x", "doing", 3)];
  const cols = boardColumns(tasks);
  assert.deepEqual(cols.map((c) => c.status), ["todo", "doing", "done"]);
  assert.deepEqual(cols.map((c) => c.tasks.map((k) => k.id)), [["a", "b"], ["x"], ["d2", "d1"]]);
  assert.deepEqual(boardColumns(tasks, true).map((c) => c.status), ["todo", "doing"]);
});

test("cleanBlockState keeps only hideDone: true", () => {
  assert.deepEqual(cleanBlockState({ hideDone: true, junk: 1 }), { hideDone: true });
  for (const bad of [null, undefined, "x", { hideDone: "yes" }, {}]) assert.deepEqual(cleanBlockState(bad), {});
});
