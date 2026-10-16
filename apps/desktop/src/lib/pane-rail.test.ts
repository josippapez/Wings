import assert from "node:assert/strict";
import { test } from "node:test";
import { cleanPaneRail, nearestCorner, railProgress } from "./pane-rail.ts";

const pages = ["board"];
const bad = (raw: unknown, message: RegExp) => assert.throws(() => cleanPaneRail(raw, pages), message);

test("a rail with items and an open page is kept, unknown fields dropped", () => {
  const rail = { title: "Todo", items: [{ text: "a", status: "done" as const }, { text: "b", status: "in_progress" as const }], open: "board" };
  assert.deepEqual(cleanPaneRail({ ...rail, extra: 1 }, pages), rail);
  assert.deepEqual(cleanPaneRail({ title: "Todo", items: [] }, pages), { title: "Todo", items: [] });
});

test("a rail that breaks a limit is refused with the reason", () => {
  bad(null, /must be an object/);
  bad({ title: "", items: [] }, /title must be a non-empty/);
  bad({ title: "x".repeat(41), items: [] }, /title is over 40/);
  bad({ title: "t", items: Array.from({ length: 31 }, () => ({ text: "a", status: "done" })) }, /at most 30/);
  bad({ title: "t", items: [{ text: "x".repeat(201), status: "done" }] }, /items\[0\]\.text is over 200/);
  bad({ title: "t", items: [{ text: "a", status: "open" }] }, /status must be one of/);
  bad({ title: "t", items: [], open: "nope" }, /open must be the id.*\(board\)/);
});

test("the card snaps to the nearest corner, and the pill counts what is done", () => {
  assert.equal(nearestCorner(10, 10, 400, 300), "top-left");
  assert.equal(nearestCorner(390, 20, 400, 300), "top-right");
  assert.equal(nearestCorner(30, 290, 400, 300), "bottom-left");
  assert.equal(nearestCorner(300, 200, 400, 300), "bottom-right");
  assert.equal(railProgress({ title: "t", items: [{ text: "a", status: "done" }, { text: "b", status: "pending" }] }), "1/2");
});
