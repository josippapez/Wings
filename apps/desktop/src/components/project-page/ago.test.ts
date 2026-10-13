/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { ago } from "./ago.ts";

const now = Date.UTC(2026, 9, 13, 12);

test("under a minute is just now", () => assert.equal(ago(now - 20_000, now), "just now"));
test("counts minutes, hours and days back", () => {
  assert.equal(ago(now - 5 * 60_000, now), "5 minutes ago");
  assert.equal(ago(now - 3 * 3_600_000, now), "3 hours ago");
  assert.equal(ago(now - 2 * 86_400_000, now), "2 days ago");
});
test("one day back is yesterday", () => assert.equal(ago(now - 86_400_000, now), "yesterday"));
