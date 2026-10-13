import test from "node:test";
import assert from "node:assert/strict";
import { applyOp, openTasks, LIMITS } from "./tasks.ts";

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
