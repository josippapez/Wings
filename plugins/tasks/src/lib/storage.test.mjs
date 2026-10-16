import test from "node:test";
import assert from "node:assert/strict";
import { ISSUE_BUDGET, LIMITS } from "./domain.ts";
import { applyOp } from "./ops.ts";
import { estimateBytes, migrateLegacy, migrateTasks, migrateV2, readStore, slotOf, writeStore } from "./storage.ts";
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

/** A wings whose storage is a Map, recording each set and delete. */
function memory(initial = {}) {
  const stored = new Map(Object.entries(initial).map(([k, v]) => [k, structuredClone(v)]));
  const writes = [];
  globalThis.wings = {
    storage: {
      get: async (key) => structuredClone(stored.get(key) ?? null),
      set: async (key, value) => (writes.push(["set", key]), void stored.set(key, JSON.parse(JSON.stringify(value)))),
      delete: async (key) => (writes.push(["delete", key]), void stored.delete(key)),
      entries: async (prefix) => [...stored].filter(([k]) => k.startsWith(prefix)).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, v]) => [k, structuredClone(v)]),
    },
  };
  return { stored, writes };
}

const PATH = "/work/web";
const at = (key) => `issues/${slotOf(PATH)}${key ? `/${key}` : ""}`;

test("slot: 16 hex digits, the same for the same path and different for others, so keys stay short for any path", () => {
  const long = `/Users/me/${"deep/".repeat(60)}project`;
  assert.match(slotOf(long), /^[0-9a-f]{16}$/);
  assert.equal(slotOf(PATH), slotOf("/work/web"));
  const slots = new Set(Array.from({ length: 2000 }, (_, i) => slotOf(`/work/project-${i}`)));
  assert.equal(slots.size, 2000);
  assert.ok(`issues/${slotOf(long)}/ABC-999999999`.length <= 128);
});

test("migration: a project in one entry (format 3 or 2) or old tasks becomes one entry per issue, and the old entries go", async () => {
  let s = fresh();
  for (const t of ["a", "b", "c"]) s = create(s, t).store;
  for (const [initial, keys] of [
    [{ [`issues:${PATH}`]: s }, ["WEB-1", "WEB-2", "WEB-3"]],
    [{ [`issues:${PATH}`]: { ...s, v: 2, issues: s.issues.map(({ type, ...i }) => i) } }, ["WEB-1", "WEB-2", "WEB-3"]],
    [{ [`tasks:${PATH}`]: [{ title: "x", status: "todo", createdAt: 1 }] }, ["WEB-1"]],
  ]) {
    const { stored } = memory({ ...initial, "issues:/other": s });
    const before = await readStore(PATH, "web");
    assert.equal(await migrateLegacy(PATH, "web"), true);
    assert.deepEqual([...stored.keys()].sort(), ["issues:/other", at(), ...keys.map(at)].sort());
    assert.deepEqual(stored.get(at()), { v: 3, path: PATH, prefix: "WEB", next: keys.length + 1 });
    assert.deepEqual(await readStore(PATH, "web"), before);
    assert.equal(await migrateLegacy(PATH, "web"), false, "once");
  }
  memory();
  assert.equal(await migrateLegacy(PATH, "web"), false, "nothing to migrate");
});

test("migration cut short: until the project's entry is written, pages read the old entry and the next start migrates again", async () => {
  let s = fresh();
  for (const t of ["a", "b"]) s = create(s, t).store;
  const { stored } = memory({ [`issues:${PATH}`]: s, [at("WEB-1")]: { ...s.issues[0], title: "half" } });
  assert.equal((await readStore(PATH, "web")).issues[0].title, "a");
  await migrateLegacy(PATH, "web");
  assert.equal(stored.get(at("WEB-1")).title, "a");
  assert.equal((await readStore(PATH, "web")).issues.length, 2);
});

test("writes: a note saves only its issue, a create saves the counter before the issue, a removal deletes its entry", async () => {
  const { stored, writes } = memory();
  const save = async (op) => {
    const before = await readStore(PATH, "web");
    const { store } = applyOp(before, op, "you", 5);
    writes.length = 0;
    await writeStore(PATH, before, store);
    return writes.slice();
  };
  assert.deepEqual(await save({ kind: "create", title: "a" }), [["set", at()], ["set", at("WEB-1")]]);
  await save({ kind: "create", title: "b" });
  await save({ kind: "create", title: "c" });
  assert.deepEqual(await save({ kind: "note", key: "WEB-2", text: "next" }), [["set", at("WEB-2")]]);
  assert.deepEqual(await save({ kind: "update", key: "WEB-1", relatedAdd: ["WEB-3"] }), [["set", at("WEB-1")], ["set", at("WEB-3")]]);
  assert.deepEqual(await save({ kind: "remove", key: "WEB-3" }), [["set", at("WEB-1")], ["delete", at("WEB-3")]]);
  const read = await readStore(PATH, "web");
  assert.deepEqual(read.issues.map((i) => i.key), ["WEB-1", "WEB-2"]);
  assert.equal(read.next, 4);
  assert.equal(stored.get(at()).next, 4);
});

test("issues read back in key order, past WEB-9, and another path in the same slot is refused, not merged", async () => {
  let s = fresh();
  for (let i = 0; i < 11; i++) s = create(s, `t${i}`).store;
  const { stored } = memory();
  await writeStore(PATH, fresh(), s);
  assert.deepEqual((await readStore(PATH, "web")).issues.map((i) => i.key), s.issues.map((i) => i.key));
  stored.set(at(), { ...stored.get(at()), path: "/elsewhere" });
  await assert.rejects(readStore(PATH, "web"), /Issues keeps \/elsewhere where \/work\/web would go/);
});

test("size: an issue with a full description and 48 full notes fits in one entry, one that would go over is refused and nothing is saved", async () => {
  let s = create(fresh(), "big", { description: "d".repeat(LIMITS.description) }).store;
  const note = (store) => applyOp(store, { kind: "note", key: "WEB-1", text: "n".repeat(LIMITS.note) }, "you", 2).store;
  for (let i = 0; i < 48; i++) s = note(s);
  const { stored } = memory();
  await writeStore(PATH, fresh(), s);
  assert.ok(estimateBytes(s.issues[0]) < ISSUE_BUDGET);
  const full = note(note(note(s)));
  await assert.rejects(writeStore(PATH, s, full), /WEB-1 would take 1\d{3} KB, over the 1000 KB one issue may use/);
  assert.equal(stored.get(at("WEB-1")).log.length, 49);
});

test("size: 200 issues with 10 log entries each are about 2 KB an issue, and the estimate counts UTF-8 bytes", () => {
  let s = fresh();
  const sess = { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "checkout summary" };
  for (let i = 0; i < 200; i++) {
    s = create(s, `Issue number ${i} with a title of a normal length`, { description: "Goal: do the thing.\n\nScope: this and that.\n\nDone when: it works and has a test." }, "you", i).store;
    for (let j = 0; j < 9; j++) s = applyOp(s, { kind: "note", key: `WEB-${i + 1}`, text: "Did the next step of the work and checked that it still passes. Next: the follow-up." }, sess, i + j).store;
  }
  const bytes = estimateBytes(s);
  console.log(`# 200 issues x 10 log entries: ${bytes} bytes (${(bytes / 1000).toFixed(0)} KB, ${(bytes / 200).toFixed(0)} bytes per issue)`);
  assert.ok(bytes / 200 < 2_500);
  assert.equal(estimateBytes({ ...fresh(), prefix: "é" }), new TextEncoder().encode(JSON.stringify({ ...fresh(), prefix: "é" })).length);
});
