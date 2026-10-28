/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { agentSplit, findPane, insertPane, mapPanes, pane, paneIds, paneNames, remove, renamePane, setDir, split, without } from "./layout.ts";

test("a pane's name is set, read, and cleared by a blank one", () => {
  let tree = split(pane("a"), "a", "row", "b");
  tree = renamePane(tree, "b", "  build  ");
  assert.deepEqual(paneNames(tree), { b: "build" });
  assert.deepEqual(paneNames(renamePane(tree, "b", "  ")), {});
});

test("names survive giving panes fresh ids on restore", () => {
  const tree = renamePane(split(pane("a"), "a", "row", "b"), "a", "api");
  assert.deepEqual(paneNames(mapPanes(tree, (id) => `${id}2`)), { a2: "api" });
});

test("moving a pane out and into another tab keeps its name", () => {
  const source = renamePane(split(pane("a"), "a", "row", "b"), "b", "logs");
  const leaf = findPane(source, "b")!;
  const rest = remove(source, "b")!;
  const target = insertPane(pane("x"), "x", "row", leaf);
  assert.deepEqual(paneIds(rest), ["a"]);
  assert.deepEqual(paneIds(target), ["x", "b"]);
  assert.deepEqual(paneNames(target), { b: "logs" });
});

test("an agent's terminals sit in one split that can turn from below to beside, or be left out of the tree", () => {
  // Below the agent first, the next beside the last one.
  let tree = split(pane("agent"), "agent", "column", "c1", 0.65);
  tree = split(tree, "c1", "row", "c2");
  const cmds = new Set(["c1", "c2"]);
  const at = agentSplit(tree, "agent", cmds)!;
  assert.equal(at.dir, "column");
  const beside = setDir(tree, at.id, "row");
  assert.equal(agentSplit(beside, "agent", cmds)!.dir, "row");
  assert.deepEqual(paneIds(without(beside, cmds)), ["agent"]);
  assert.deepEqual(paneIds(without(pane("c1"), cmds)), ["c1"]);
  assert.equal(agentSplit(tree, "agent", new Set(["other"])), null);
});
