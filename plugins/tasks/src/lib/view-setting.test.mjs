import test from "node:test";
import assert from "node:assert/strict";
import { defaultView, startLayout } from "./view-setting.ts";

test("default view: board unless the setting says rows or list, and each starts the tab in its layout", async () => {
  let stored;
  globalThis.wings = { storage: { get: async () => stored ?? null } };
  for (const [value, view] of [[undefined, "board"], ["grid", "board"], [5, "board"], ["rows", "rows"], ["list", "list"]]) {
    stored = value;
    assert.equal(await defaultView.get(), view, `${value}`);
  }
  assert.deepEqual(startLayout("board"), { layout: "board", byParent: false });
  assert.deepEqual(startLayout("rows"), { layout: "board", byParent: true });
  assert.deepEqual(startLayout("list"), { layout: "list", byParent: false });
});
