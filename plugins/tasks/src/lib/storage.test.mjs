import test from "node:test";
import assert from "node:assert/strict";
import { BUDGET } from "./domain.ts";
import { applyOp } from "./ops.ts";
import { estimateBytes, migrateTasks, migrateV2 } from "./storage.ts";
import { create, fresh } from "./testing.mjs";

test("migration: tasks become issues, oldest first, notes and doing mapped, branch kept", () => {
  const store = migrateTasks(
    [
      { id: "b", title: "Second", notes: "n", status: "doing", links: { branch: "feat/x" }, createdAt: 20, updatedAt: 30 },
      { id: "a", title: "First", status: "todo", createdAt: 10, updatedAt: 10 },
      { id: "c", title: "Third", status: "done", createdAt: 30, updatedAt: 40 },
      { junk: true },
    ],
    "Web Shop",
  );
  assert.equal(store.v, 3);
  assert.equal(store.prefix, "WEB");
  assert.equal(store.next, 4);
  assert.deepEqual(store.issues.map((i) => [i.key, i.title, i.status]), [["WEB-1", "First", "todo"], ["WEB-2", "Second", "in_progress"], ["WEB-3", "Third", "done"]]);
  const second = store.issues[1];
  assert.equal(second.description, "n");
  assert.deepEqual(second.links, { branch: "feat/x" });
  assert.equal(second.updatedAt, 30);
  assert.deepEqual(second.log, [{ at: 20, by: "you", kind: "created", text: "Imported from Tasks" }]);
  assert.deepEqual(migrateTasks(null, "x").issues, []);
});

test("type: new issues are tasks, a v2 store migrates to v3 with every issue a task, old Tasks still migrate", () => {
  assert.equal(create(fresh(), "a").store.issues[0].type, "task");
  const v2 = { v: 2, prefix: "WEB", next: 3, issues: [{ key: "WEB-1", title: "a", status: "todo", sessions: [], log: [], createdAt: 1, updatedAt: 1 }, { key: "WEB-2", title: "b", status: "done", sessions: [], log: [], createdAt: 1, updatedAt: 1 }] };
  const m = migrateV2(v2);
  assert.equal(m.v, 3);
  assert.deepEqual(m.issues.map((i) => i.type), ["task", "task"]);
  assert.equal(m.next, 3);
  assert.equal(migrateV2({ ...v2, v: 3 }), null);
  assert.equal(migrateTasks([{ title: "x", status: "todo", createdAt: 1 }], "web").issues[0].type, "task");
});

test("size: 200 issues with 10 log entries each fit well inside the budget, and the estimate counts UTF-8 bytes", () => {
  let s = fresh();
  const sess = { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "checkout summary" };
  for (let i = 0; i < 200; i++) {
    s = create(s, `Issue number ${i} with a title of a normal length`, { description: "Goal: do the thing.\n\nScope: this and that.\n\nDone when: it works and has a test." }, "you", i).store;
    for (let j = 0; j < 9; j++) s = applyOp(s, { kind: "note", key: `WEB-${i + 1}`, text: "Did the next step of the work and checked that it still passes. Next: the follow-up." }, sess, i + j).store;
  }
  const bytes = estimateBytes(s);
  console.log(`# 200 issues x 10 log entries: ${bytes} bytes (${(bytes / 1000).toFixed(0)} KB, ${(bytes / 200).toFixed(0)} bytes per issue)`);
  assert.ok(bytes < BUDGET, "inside the per-project budget");
  assert.ok(bytes < 1_000_000 / 2, "leaves room for other projects in the 1 MB");
  assert.equal(estimateBytes({ ...fresh(), prefix: "é" }), new TextEncoder().encode(JSON.stringify({ ...fresh(), prefix: "é" })).length);
});
