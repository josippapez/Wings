import assert from "node:assert/strict";
import { test } from "node:test";
import { cleanPaneStatus } from "./pane-status.ts";
import { requireUi } from "./plugin-pages.ts";
import { withBadge } from "./badges.ts";

const pages = ["hud", "board"];
const bad = (raw: unknown, message: RegExp) => assert.throws(() => cleanPaneStatus(raw, pages), message);

test("a status with items, rows and an open page is kept as given", () => {
  const status = {
    items: [{ label: "5h", value: "8% · 4h19m", percent: 8, tone: "warning" as const }, { value: "2 failed", tone: "danger" as const }],
    rows: [{ label: "Context", value: "41%", tone: "info" as const }],
    open: "hud",
  };
  assert.deepEqual(cleanPaneStatus(status, pages), status);
});

test("unknown fields are dropped", () => {
  assert.deepEqual(cleanPaneStatus({ items: [{ value: "x", extra: 1 }], extra: 2 }, pages), { items: [{ value: "x" }] });
});

test("a status is refused over the item, label and value limits", () => {
  const item = { value: "1" };
  assert.equal(cleanPaneStatus({ items: [item, item, item, item] }, pages).items.length, 4);
  bad({ items: [item, item, item, item, item] }, /at most 4 items/);
  bad({ items: [] }, /non-empty array/);
  bad({ items: [{ label: "x".repeat(17), value: "1" }] }, /label is over 16/);
  assert.ok(cleanPaneStatus({ items: [{ label: "x".repeat(16), value: "y".repeat(32) }] }, pages));
  bad({ items: [{ value: "y".repeat(33) }] }, /value is over 32/);
  bad({ items: [{ value: "" }] }, /value must be a non-empty string/);
  bad({ items: [{}] }, /value is required/);
});

test("percent must be a number from 0 to 100 and tone a known tone", () => {
  assert.ok(cleanPaneStatus({ items: [{ value: "1", percent: 0 }, { value: "1", percent: 100 }] }, pages));
  for (const percent of [-1, 101, NaN, "5"]) bad({ items: [{ value: "1", percent }] }, /percent must be a number from 0 to 100/);
  bad({ items: [{ value: "1", tone: "red" }] }, /tone must be one of/);
});

test("rows are limited like a badge's, and open must be one of the plugin's pages", () => {
  const row = { label: "a", value: "b" };
  bad({ items: [{ value: "1" }], rows: Array(13).fill(row) }, /at most 12 rows/);
  bad({ items: [{ value: "1" }], rows: [{ label: "a" }] }, /rows\[0\]\.value is required/);
  bad({ items: [{ value: "1" }], open: "nope" }, /open must be the id of one of the plugin's sidebars or views \(hud, board\)/);
  bad("text", /must be an object or null/);
  bad(null, /must be an object or null/);
});

test("setPaneStatus is refused without status in contributes.ui, like a badge without badges", () => {
  assert.throws(() => requireUi("hud", ["badges"], "status"), /hud must declare "status" in contributes\.ui/);
  assert.doesNotThrow(() => requireUi("hud", ["status"], "status"));
});

test("a status is stored one per plugin per pane, and null removes it", () => {
  const a = { items: [{ value: "1" }] };
  const all = withBadge({}, "p1", "hud", a);
  assert.equal(withBadge(all, "p1", "hud", { items: [{ value: "1" }] }), all);
  assert.deepEqual(withBadge(all, "p1", "hud", null), { p1: {} });
});
