/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { parseNotes } from "./release-notes.ts";

test("dash lines become one list, other lines paragraphs, in order", () => {
  assert.deepEqual(parseNotes("Fixes\n- one\n- two\n\nThanks <b>all</b>"), [
    { list: false, text: "Fixes" },
    { list: true, items: ["one", "two"] },
    { list: false, text: "Thanks <b>all</b>" },
  ]);
});

test("missing or blank notes give no blocks", () => {
  assert.deepEqual(parseNotes(undefined), []);
  assert.deepEqual(parseNotes("  \n"), []);
});
