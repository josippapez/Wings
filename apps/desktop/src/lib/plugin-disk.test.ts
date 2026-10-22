/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { diskFindRequest, diskUsageRequest } from "./plugin-disk.ts";

const ok = { path: "/", token: "abc" };

test("diskUsage sends what the plugin gave in the range Rust reads, and null for what it left out", () => {
  assert.deepEqual(diskUsageRequest(ok), { path: "/", minBytes: null, depth: null, maxEntries: null, token: "abc", stream: false });
  assert.deepEqual(diskUsageRequest({ ...ok, minBytes: 1.5, depth: 3, maxEntries: 10, stream: true }), { path: "/", minBytes: 2, depth: 3, maxEntries: 10, token: "abc", stream: true });
  assert.equal(diskUsageRequest({ ...ok, depth: 10 ** 12 }).depth, 2 ** 32 - 1);
  assert.equal(diskUsageRequest({ ...ok, minBytes: 1e300 }).minBytes, Number.MAX_SAFE_INTEGER);
  for (const bad of [{ path: "" }, { path: 5 }, { token: "" }, { token: "x".repeat(65) }, { minBytes: -1 }, { minBytes: NaN }, { minBytes: "5" }, { depth: 1.5 }, { depth: -1 }, { maxEntries: Infinity }]) {
    assert.throws(() => diskUsageRequest({ ...ok, ...bad }), Error, JSON.stringify(bad));
  }
});

test("diskFind takes 1 to 16 names and up to 32 to skip", () => {
  assert.deepEqual(diskFindRequest({ ...ok, names: [".git"] }), { path: "/", names: [".git"], skip: [], token: "abc", stream: false });
  assert.deepEqual(diskFindRequest({ ...ok, names: [".git", "Cargo.toml"], skip: ["node_modules", ".Trash"], stream: true }), {
    path: "/",
    names: [".git", "Cargo.toml"],
    skip: ["node_modules", ".Trash"],
    token: "abc",
    stream: true,
  });
  const many = (n: number) => Array.from({ length: n }, (_, i) => `n${i}`);
  assert.equal(diskFindRequest({ ...ok, names: many(16), skip: many(32) }).names.length, 16);
  assert.equal(diskFindRequest({ ...ok, names: ["x".repeat(255)] }).names[0].length, 255);
});

test("diskFind refuses names that can't be a file name, and lists past the limits", () => {
  const refused = (extra: Record<string, unknown>) => assert.throws(() => diskFindRequest({ ...ok, names: [".git"], ...extra }), Error, JSON.stringify(extra).slice(0, 60));
  const many = (n: number) => Array.from({ length: n }, (_, i) => `n${i}`);
  assert.throws(() => diskFindRequest({ ...ok }), { message: "diskFind names must be a list of 1 to 16 names" });
  assert.throws(() => diskFindRequest({ ...ok, names: [] }), { message: "diskFind names must be a list of 1 to 16 names" });
  assert.throws(() => diskFindRequest({ ...ok, names: many(17) }), { message: "diskFind names must be a list of 1 to 16 names" });
  assert.throws(() => diskFindRequest({ ...ok, names: [""] }), { message: "diskFind names must be names of 1 to 255 characters with no /" });
  assert.throws(() => diskFindRequest({ ...ok, names: ["a/b"] }), { message: "diskFind names must be names of 1 to 255 characters with no /" });
  assert.throws(() => diskFindRequest({ ...ok, names: ["x".repeat(256)] }), Error);
  assert.throws(() => diskFindRequest({ ...ok, names: [".git", 5] }), Error);
  assert.throws(() => diskFindRequest({ ...ok, names: ".git" }), Error);
  refused({ skip: many(33) });
  refused({ skip: [""] });
  refused({ skip: ["node_modules/x"] });
  refused({ skip: "node_modules" });
  refused({ path: "" });
  refused({ token: "" });
});
