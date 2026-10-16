import assert from "node:assert/strict";
import { test } from "node:test";

import { sidebarPageShown } from "./sidebar-page.ts";

test("a closed sidebar shows no page, even the one it keeps for the slide", () => {
  assert.equal(sidebarPageShown(false, "a", "a"), false);
});

test("a docked or peeking sidebar shows only its current page", () => {
  assert.equal(sidebarPageShown(true, "a", "a"), true);
  assert.equal(sidebarPageShown(true, "b", "a"), false);
  assert.equal(sidebarPageShown(true, "a", undefined), false);
});
