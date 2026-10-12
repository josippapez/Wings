import assert from "node:assert/strict";
import { test } from "node:test";
import { withBadge, withLabel } from "./badges.ts";

test("withBadge keeps the same object for a badge the pane already has, and replaces a changed one", () => {
  const all = { p1: { pr: { text: "#12", tone: "ok" } } };
  assert.equal(withBadge(all, "p1", "pr", { text: "#12", tone: "ok" }), all);
  assert.equal(withBadge(all, "p2", "pr", null), all);
  assert.deepEqual(withBadge(all, "p1", "pr", { text: "#12", tone: "warn" }), { p1: { pr: { text: "#12", tone: "warn" } } });
  assert.deepEqual(withBadge(all, "p1", "pr", null), { p1: {} });
});

test("withLabel keeps the same object for an unchanged label, and removes a cleared one", () => {
  const all = { "hud:main": { text: "3" } };
  assert.equal(withLabel(all, "hud:main", { text: "3" }), all);
  assert.equal(withLabel(all, "hud:other", null), all);
  assert.deepEqual(withLabel(all, "hud:main", null), {});
  assert.deepEqual(withLabel(all, "hud:main", { text: "4" }), { "hud:main": { text: "4" } });
});
