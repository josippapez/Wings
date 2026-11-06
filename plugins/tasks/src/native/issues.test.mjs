import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { buildNative } from "../../build-native.mjs";
import { LIMITS, STATUSES, TYPES } from "../lib/domain.ts";
import { entriesOf, slotOf } from "../lib/storage.ts";
import { create, fresh } from "../lib/testing.mjs";

const project = { id: "/work/web", name: "web" };
const key = `issues/${slotOf(project.id)}`;
const bundle = readFileSync(new URL("../../dist/native-issues.js", import.meta.url), "utf8");
const plain = (value) => JSON.parse(JSON.stringify(value));

function boot(entries = [], fail) {
  const stored = new Map(structuredClone(entries));
  const reads = [];
  const storage = Object.freeze({
    get: async (key) => {
      reads.push(["get", key]);
      if (fail) throw new Error(fail);
      return structuredClone(stored.get(key) ?? null);
    },
    entries: async (prefix) => {
      reads.push(["entries", prefix]);
      if (fail) throw new Error(fail);
      return [...stored].filter(([key]) => key.startsWith(prefix)).sort(([a], [b]) => a < b ? -1 : 1).map(([k, v]) => [k, structuredClone(v)]);
    },
  });
  const context = vm.createContext({ wings: Object.freeze({ storage }) });
  vm.runInContext(bundle, context);
  assert.deepEqual(reads, [], "loading the IIFE does not read storage or register browser work");
  return { stored, reads, screen: async (p = project, action = null) => plain(await context.wingsNative(p, action)) };
}

function storeOf(count = 1) {
  let store = fresh();
  for (let i = 0; i < count; i++) store = create(store, `Issue ${i + 1}`, { status: STATUSES[i % STATUSES.length], type: TYPES[i % TYPES.length] }).store;
  return store;
}

test("native bundle matches the separate build and has no browser or React runtime", async () => {
  assert.equal(await buildNative(), bundle);
  assert.doesNotMatch(bundle, /\b(?:window|document|React|require)\b/);
  assert.match(bundle, /globalThis\.wingsNative\s*=/);
  const screen = await boot().screen();
  assert.deepEqual(screen, { version: 1, title: "Issues", state: "empty", rows: [], actions: [{ id: "refresh", label: "Refresh" }] });
});

test("native current store preserves issue titles, status, type and numeric key order without writes", async () => {
  const store = storeOf(11);
  store.issues[0].title = "<b>Plain issue text</b>";
  const other = { id: "/work/other", name: "other" };
  const { stored, screen } = boot([...entriesOf(project.id, store), ...entriesOf(other.id, storeOf(1))]);
  const before = JSON.stringify([...stored]);
  const result = await screen();
  assert.equal(result.state, "ready");
  assert.deepEqual(result.rows, store.issues.map((issue) => ({ id: issue.key, title: issue.title, status: issue.status, type: issue.type })));
  assert.equal(result.rows[9].id, "WEB-10");
  assert.equal((await screen(other)).rows.length, 1);
  assert.equal((await screen({ id: "/work/absent", name: "absent" })).state, "empty");
  assert.equal(JSON.stringify([...stored]), before);
});

test("native reads single-entry Issues v3 and v2 through existing migrations, preserving stored order", async () => {
  const store = storeOf(3);
  store.issues.reverse();
  for (const value of [store, { ...store, v: 2, issues: store.issues.map(({ type, ...issue }) => issue) }]) {
    const { stored, screen } = boot([[`issues:${project.id}`, value]]);
    const before = JSON.stringify([...stored]);
    const result = await screen();
    assert.equal(result.state, "ready");
    assert.deepEqual(result.rows.map((row) => row.id), ["WEB-3", "WEB-2", "WEB-1"]);
    assert.deepEqual(result.rows.map((row) => row.type), value.v === 2 ? ["task", "task", "task"] : ["feature", "bug", "task"]);
    assert.equal(JSON.stringify([...stored]), before);
  }
});

test("native Tasks migration uses original chronology, title limit and doing/done status mapping without saving", async () => {
  const tasks = [
    { title: "Last", status: "done", createdAt: 30 },
    { title: "x".repeat(LIMITS.title + 5), status: "todo", createdAt: 10 },
    { title: "Middle", status: "doing", createdAt: 20 },
  ];
  const { stored, screen } = boot([[`tasks:${project.id}`, tasks]]);
  const before = JSON.stringify([...stored]);
  assert.deepEqual((await screen()).rows, [
    { id: "WEB-1", title: "x".repeat(LIMITS.title), status: "todo", type: "task" },
    { id: "WEB-2", title: "Middle", status: "in_progress", type: "task" },
    { id: "WEB-3", title: "Last", status: "done", type: "task" },
  ]);
  assert.equal(JSON.stringify([...stored]), before);
});

test("native current metadata wins over old lists, while partial per-issue migration still reads the old list", async () => {
  const store = storeOf(1);
  const current = entriesOf(project.id, store);
  assert.equal((await boot([...current, [`issues:${project.id}`, "corrupt retired data"]]).screen()).state, "ready");
  const partial = boot([[`${key}/WEB-1`, { ...store.issues[0], title: "partial" }], [`issues:${project.id}`, store]]);
  assert.equal((await partial.screen()).rows[0].title, "Issue 1");
});

test("native Refresh rereads storage and refuses unknown actions before any reads", async () => {
  const { stored, reads, screen } = boot(entriesOf(project.id, storeOf(1)));
  assert.equal((await screen(project, "edit")).state, "error");
  assert.deepEqual(reads, []);
  assert.equal((await screen()).rows.length, 1);
  for (const [k, v] of entriesOf(project.id, storeOf(2))) stored.set(k, v);
  assert.equal((await screen(project, "refresh")).rows.length, 2);
  assert.equal((await screen(null)).state, "error");
  assert.equal((await screen({ name: "web" })).state, "error");
});

test("native storage failures and malformed formats are error screens, never empty lists", async () => {
  const cases = [
    [[key, null]],
    [[key, { v: 3, path: project.id, prefix: "WEB", next: "bad" }]],
    [[`issues:${project.id}`, { v: 99, issues: [], prefix: "WEB", next: 1 }]],
    [[`issues:${project.id}`, { v: 3, issues: "bad", prefix: "WEB", next: 1 }]],
    [[`issues:${project.id}`, "broken"], [`tasks:${project.id}`, []]],
    [[`tasks:${project.id}`, { broken: true }]],
    [[`tasks:${project.id}`, [{ broken: true }]]],
    [[key, { v: 3, path: "/somewhere/else", prefix: "WEB", next: 1 }]],
  ];
  for (const entries of cases) {
    const { stored, screen } = boot(entries);
    const before = JSON.stringify([...stored]);
    const result = await screen();
    assert.equal(result.state, "error", JSON.stringify(entries));
    assert.deepEqual(result.rows, []);
    assert.ok(result.error);
    assert.equal(JSON.stringify([...stored]), before);
  }
  const failed = await boot([], "Disk read failed").screen();
  assert.equal(failed.state, "error");
  assert.match(failed.error, /Disk read failed/);
  assert.equal((await boot([], "x".repeat(501)).screen()).error.length, 500);
});

test("native bounds and validates rows without truncating issue data", async () => {
  const issue = storeOf().issues[0];
  for (const broken of [null, { ...issue, key: "bad" }, { ...issue, title: "" }, { ...issue, title: "x".repeat(201) }, { ...issue, status: "doing" }, { ...issue, type: "other" }]) {
    const result = await boot([[key, { v: 3, path: project.id, prefix: "WEB", next: 2 }], [`${key}/WEB-1`, broken]]).screen();
    assert.equal(result.state, "error");
    assert.deepEqual(result.rows, []);
  }
  const old = { ...fresh(), issues: [{ ...issue, title: "x".repeat(200) }] };
  assert.equal((await boot([[`issues:${project.id}`, old]]).screen()).rows[0].title.length, 200);
  const issues = Array.from({ length: 2_001 }, (_, i) => ({ ...issue, key: `WEB-${i + 1}` }));
  const full = boot([[`issues:${project.id}`, { ...fresh(), issues: issues.slice(0, 2_000) }]]);
  assert.equal((await full.screen()).rows.length, 2_000);
  const overflow = await boot([[`issues:${project.id}`, { ...fresh(), issues }]]).screen();
  assert.equal(overflow.state, "error");
  assert.match(overflow.error, /at most 2000/);
  assert.deepEqual(overflow.rows, []);
  const duplicate = await boot([[`issues:${project.id}`, { ...fresh(), issues: [issue, issue] }]]).screen();
  assert.equal(duplicate.state, "error");
  assert.match(duplicate.error, /duplicate/);
});
