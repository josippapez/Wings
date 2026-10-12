/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { moveTab, reorder, sortByOrder } from "./tab-order.ts";

const tab = (id: string, spaceId: string) => ({ id, spaceId });
const ids = (list: { id: string }[]) => list.map((t) => t.id);

test("reorder moves one item and clamps the target", () => {
  assert.deepEqual(reorder(["a", "b", "c"], 0, 2), ["b", "c", "a"]);
  assert.deepEqual(reorder(["a", "b", "c"], 2, 0), ["c", "a", "b"]);
  assert.deepEqual(reorder(["a", "b", "c"], 0, 9), ["b", "c", "a"]);
});

test("reorder returns the same list when nothing moves", () => {
  const list = ["a", "b"];
  assert.equal(reorder(list, 1, 1), list);
  assert.equal(reorder(list, 5, 0), list);
});

test("moveTab reorders inside its project and leaves other projects' places alone", () => {
  const tabs = [tab("a1", "A"), tab("b1", "B"), tab("a2", "A"), tab("b2", "B"), tab("a3", "A")];
  assert.deepEqual(ids(moveTab(tabs, "a3", 0)), ["a3", "b1", "a1", "b2", "a2"]);
  assert.deepEqual(ids(moveTab(tabs, "a1", 1)), ["a2", "b1", "a1", "b2", "a3"]);
});

test("moveTab with an unknown id or the same place changes nothing", () => {
  const tabs = [tab("a1", "A"), tab("a2", "A")];
  assert.equal(moveTab(tabs, "zz", 0), tabs);
  assert.equal(moveTab(tabs, "a1", 0), tabs);
});

test("sortByOrder follows the saved ids and puts new ones last", () => {
  const items = [{ id: "a" }, { id: "b" }, { id: "c" }, { id: "d" }];
  assert.deepEqual(ids(sortByOrder(items, ["c", "a", "gone"])), ["c", "a", "b", "d"]);
  assert.equal(sortByOrder(items, undefined), items);
});
