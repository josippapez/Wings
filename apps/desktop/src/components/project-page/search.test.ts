import assert from "node:assert/strict";
import { test } from "node:test";

import type { Info } from "@/lib/project-info";

import { clampMatch, findMatches, stepMatch } from "./search.ts";

const info: Info = {
  v: 1,
  updatedAt: 0,
  facts: [
    { key: "Dev server", value: "pnpm dev, port 5173", by: "you", updatedAt: 0 },
    { key: "Deploy", value: "Run `PNPM build` first", by: "agent", updatedAt: 0 },
  ],
  items: [
    { kind: "section", id: "s1", title: "Setup", body: "Install with **pnpm** install", by: "you", updatedAt: 0 },
    { kind: "section", id: "s2", title: "PNPM tips", body: "", by: "you", updatedAt: 0 },
    { kind: "block", id: "b1", pluginId: "tasks", blockId: "board", state: null, height: null },
    { kind: "section", id: "s3", title: "Other", body: "nothing here", by: "you", updatedAt: 0 },
  ],
};

test("matches are case-insensitive across fact keys and values, section titles and bodies, in page order", () => {
  assert.deepEqual(findMatches(info, "pnpm"), [
    { kind: "fact", key: "Dev server" },
    { kind: "fact", key: "Deploy" },
    { kind: "item", id: "s1" },
    { kind: "item", id: "s2" },
  ]);
  assert.deepEqual(findMatches(info, "DEPLOY"), [{ kind: "fact", key: "Deploy" }]);
  assert.deepEqual(findMatches(info, "board"), [{ kind: "item", id: "b1" }]);
});

test("the markdown source is searched as plain text", () => {
  assert.deepEqual(findMatches(info, "**pnpm**"), [{ kind: "item", id: "s1" }]);
  assert.equal(findMatches(info, ".*").length, 0);
});

test("a blank query or no hit matches nothing", () => {
  assert.deepEqual(findMatches(info, ""), []);
  assert.deepEqual(findMatches(info, "   "), []);
  assert.deepEqual(findMatches(info, "zzz"), []);
});

test("stepping wraps around both ways", () => {
  assert.equal(stepMatch(0, 1, 3), 1);
  assert.equal(stepMatch(2, 1, 3), 0);
  assert.equal(stepMatch(0, -1, 3), 2);
  assert.equal(stepMatch(0, 1, 0), 0);
});

test("a position past the end after matches shrink clamps to the last one", () => {
  assert.equal(clampMatch(4, 3), 2);
  assert.equal(clampMatch(1, 3), 1);
  assert.equal(clampMatch(2, 0), 0);
});
