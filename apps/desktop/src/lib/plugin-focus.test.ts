/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { FocusTracker, SETTLE_MS } from "./plugin-focus.ts";

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

test("losing focus counts once it has held, gaining it counts at once", async () => {
  const seen: boolean[] = [];
  const t = new FocusTracker(true, (f) => seen.push(f));
  t.report(false);
  assert.equal(t.focused, true);
  await wait(SETTLE_MS + 50);
  assert.deepEqual(seen, [false]);
  t.report(true);
  assert.deepEqual(seen, [false, true]);
});

test("moving between Wings windows (false, then true) never reaches plugins", async () => {
  const seen: boolean[] = [];
  const t = new FocusTracker(true, (f) => seen.push(f));
  t.report(false);
  t.report(true);
  await wait(SETTLE_MS + 50);
  assert.deepEqual(seen, []);
  t.dispose();
});
