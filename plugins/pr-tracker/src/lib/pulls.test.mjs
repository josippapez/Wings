import assert from "node:assert/strict";
import test from "node:test";
import { orderPulls } from "./pulls.ts";

test("orderPulls puts yours first and keeps each group's order", () => {
  const rows = [{ id: "a", mine: false }, { id: "b", mine: true }, { id: "c", mine: false }, { id: "d", mine: true }];
  assert.deepEqual(orderPulls(rows).map((r) => r.id), ["b", "d", "a", "c"]);
  assert.deepEqual(orderPulls([]), []);
});

test("serveCached serves a list of any age only for a pane-change redraw", async () => {
  const { serveCached } = await import("./pulls.ts");
  assert.equal(serveCached(10_000, 55_000, false), true);
  assert.equal(serveCached(55_000, 55_000, false), false);
  assert.equal(serveCached(3 * 3_600_000, 55_000, false), false);
  assert.equal(serveCached(3 * 3_600_000, 55_000, true), true);
});
