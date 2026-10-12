/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { findPane, insertPane, mapPanes, pane, paneIds, paneNames, remove, renamePane, split } from "./layout.ts";

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
