/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { burstGate, cleanSound, DEFAULT_SOUND } from "./sound.ts";

test("notifications within a second play one sound", () => {
  const due = burstGate(1000);
  assert.deepEqual([0, 300, 900, 1000, 1500].map(due), [true, false, false, true, false]);
});

test("the setting defaults to on, and unknown values fall back", () => {
  assert.deepEqual(cleanSound(undefined), DEFAULT_SOUND);
  assert.deepEqual(cleanSound({ enabled: false, sound: "pop", watching: false }), { enabled: false, sound: "pop", watching: false });
  assert.deepEqual(cleanSound({ enabled: "no", sound: "gong" }), DEFAULT_SOUND);
});
