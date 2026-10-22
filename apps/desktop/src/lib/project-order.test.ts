/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { IDLE_AFTER_MS, orderProjects, parseProjectSort, restoreLastActive, touched, TOUCH_EVERY_MS, withMissing } from "./project-order.ts";

const NOW = 10 * IDLE_AFTER_MS;
const p = (id: string, extra: Record<string, unknown> = {}) => ({ id, name: id, ...extra });
const order = (spaces: ReturnType<typeof p>[], o: Partial<Parameters<typeof orderProjects>[1]> = {}) => {
  const { shown, older } = orderProjects(spaces, { sort: "manual", lastActive: {}, now: NOW, activeId: null, busy: new Set(), ...o });
  return { shown: shown.map((r) => r.space.id), older: older.map((r) => r.space.id) };
};

test("manual keeps the saved order", () => {
  assert.deepEqual(order([p("c"), p("a"), p("b")]).shown, ["c", "a", "b"]);
});

test("added puts the newest first, and projects with no time last in their old order", () => {
  const spaces = [p("old1"), p("b", { addedAt: 100 }), p("old2"), p("c", { addedAt: 300 }), p("a", { addedAt: 200 })];
  assert.deepEqual(order(spaces, { sort: "added" }).shown, ["c", "a", "b", "old1", "old2"]);
});

test("last worked on puts the most recent first, and projects with no time last", () => {
  const spaces = [p("a"), p("b"), p("c"), p("d")];
  assert.deepEqual(order(spaces, { sort: "worked", lastActive: { b: NOW - 10, c: NOW - 5, d: NOW - 20 } }).shown, ["c", "b", "d", "a"]);
});

test("name is A to Z, ignoring case, with numbers in order", () => {
  const spaces = [p("web10"), p("beta"), p("Alpha"), p("web2")];
  assert.deepEqual(order(spaces, { sort: "name" }).shown, ["Alpha", "beta", "web2", "web10"]);
});

test("pinned projects come first and are ordered among themselves by the mode", () => {
  const spaces = [p("a"), p("z", { pinned: true }), p("m", { pinned: true }), p("b")];
  assert.deepEqual(order(spaces).shown, ["z", "m", "a", "b"]);
  assert.deepEqual(order(spaces, { sort: "name" }).shown, ["m", "z", "a", "b"]);
});

test("a worktree stays right under its project and moves with it", () => {
  const spaces = [p("b"), p("a-wt", { parent: "a" }), p("a"), p("b-wt", { parent: "b" })];
  assert.deepEqual(order(spaces, { sort: "name" }).shown, ["a", "a-wt", "b", "b-wt"]);
  assert.deepEqual(order([p("x"), p("w", { parent: "x" }), p("y", { pinned: true })]).shown, ["y", "x", "w"]);
});

test("a project untouched for over 3 days goes under Older, unless it is pinned, active or runs an agent", () => {
  const stale = NOW - IDLE_AFTER_MS - 1;
  const spaces = [p("old"), p("fresh"), p("edge"), p("pin", { pinned: true }), p("act"), p("busy"), p("none")];
  const lastActive = { old: stale, fresh: NOW - 1000, edge: NOW - IDLE_AFTER_MS, pin: stale, act: stale, busy: stale };
  const got = order(spaces, { lastActive, activeId: "act", busy: new Set(["busy"]) });
  assert.deepEqual(got.older, ["old"]);
  assert.deepEqual(got.shown, ["pin", "fresh", "edge", "act", "busy", "none"]);
});

test("Older keeps the sort mode, and a worktree follows its project there", () => {
  const stale = NOW - IDLE_AFTER_MS - 1;
  const spaces = [p("b"), p("b-wt", { parent: "b" }), p("a")];
  const got = order(spaces, { sort: "name", lastActive: { b: stale, "b-wt": stale, a: stale } });
  assert.deepEqual(got, { shown: [], older: ["a", "b", "b-wt"] });
});

test("a recent worktree, a running agent in one, or the active one keeps its project out of Older", () => {
  const stale = NOW - IDLE_AFTER_MS - 1;
  const spaces = [p("a"), p("a-wt", { parent: "a" }), p("b"), p("b-wt", { parent: "b" }), p("c"), p("c-wt", { parent: "c" })];
  const lastActive = { a: stale, "a-wt": NOW - 5, b: stale, "b-wt": stale, c: stale, "c-wt": stale };
  assert.deepEqual(order(spaces, { lastActive, busy: new Set(["b-wt"]), activeId: "c-wt" }).older, []);
});

test("touched writes a project at most once a minute and returns the same object otherwise", () => {
  const first = touched({}, ["a"], 1000);
  assert.deepEqual(first, { a: 1000 });
  assert.equal(touched(first, ["a"], 1000 + TOUCH_EVERY_MS - 1), first);
  assert.deepEqual(touched(first, ["a", "b"], 1000 + TOUCH_EVERY_MS), { a: 1000 + TOUCH_EVERY_MS, b: 1000 + TOUCH_EVERY_MS });
});

test("projects with no time are stamped now once, and saved times are restored only for known projects", () => {
  const have = { a: 5 };
  assert.deepEqual(withMissing(have, ["a", "b"], 9), { a: 5, b: 9 });
  assert.equal(withMissing(have, ["a"], 9), have);
  assert.deepEqual(restoreLastActive({ a: 1, gone: 2, b: "x", c: NaN }, ["a", "b", "c"]), { a: 1 });
  assert.deepEqual(restoreLastActive("junk", ["a"]), {});
});

test("an unknown saved sort falls back to manual", () => {
  assert.equal(parseProjectSort("name"), "name");
  assert.equal(parseProjectSort("recent"), "manual");
  assert.equal(parseProjectSort(undefined), "manual");
});
