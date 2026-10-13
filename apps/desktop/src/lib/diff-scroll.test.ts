/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { LINE_HEIGHT, rowNumber, rowTop, SEPARATOR_HEIGHT, type RowFile } from "./diff-scroll.ts";

// Old file lines 50-59 become new lines 60-71: 10 lines were added above (an earlier hunk), then 2 context, 1 deleted, 3 added, 2 context.
const file: RowFile = {
  hunks: [
    { collapsedBefore: 9, deletionStart: 10, additionStart: 10, hunkContent: [{ type: "context", lines: 2 }, { type: "change", deletions: 0, additions: 10 }, { type: "context", lines: 2 }] },
    { collapsedBefore: 36, deletionStart: 50, additionStart: 60, hunkContent: [{ type: "context", lines: 2 }, { type: "change", deletions: 1, additions: 3 }, { type: "context", lines: 2 }] },
  ],
};

test("a new-side line is its own number", () => {
  assert.deepEqual(rowNumber(file, "additions", 62), { number: 62, deleted: false });
  assert.deepEqual(rowNumber(file, "additions", 63), { number: 63, deleted: false });
});

test("an old-side context line is found by the new number it became", () => {
  assert.deepEqual(rowNumber(file, "deletions", 51), { number: 61, deleted: false });
  assert.deepEqual(rowNumber(file, "deletions", 53), { number: 65, deleted: false });
});

test("a deleted line keeps its old number and is marked deleted", () => {
  assert.deepEqual(rowNumber(file, "deletions", 52), { number: 52, deleted: true });
});

test("lines outside every hunk have no row", () => {
  assert.equal(rowNumber(file, "additions", 30), null);
  assert.equal(rowNumber(file, "deletions", 5), null);
});

const H = LINE_HEIGHT;

test("rowTop counts a separator for each gap before the row", () => {
  // Hunk 1 has 2 + 10 + 2 = 14 rows in unified view; hunk 2 starts after 2 separators.
  assert.equal(rowTop(file, "additions", 10, true), 1 * SEPARATOR_HEIGHT);
  assert.equal(rowTop(file, "additions", 60, true), 14 * H + 2 * SEPARATOR_HEIGHT);
});

test("in split view a change block is as tall as its longer side", () => {
  // Hunk 2: 2 context, then 1 deleted next to 3 added (3 rows), then 2 context.
  assert.equal(rowTop(file, "additions", 65, true), (14 + 2 + 3) * H + 2 * SEPARATOR_HEIGHT);
  assert.equal(rowTop(file, "deletions", 52, true), (14 + 2) * H + 2 * SEPARATOR_HEIGHT);
});

test("in unified view deleted lines come before added ones, and the block has both", () => {
  assert.equal(rowTop(file, "deletions", 52, false), (14 + 2) * H + 2 * SEPARATOR_HEIGHT);
  assert.equal(rowTop(file, "additions", 62, false), (14 + 2 + 1) * H + 2 * SEPARATOR_HEIGHT);
  assert.equal(rowTop(file, "additions", 65, false), (14 + 2 + 1 + 3) * H + 2 * SEPARATOR_HEIGHT);
});

test("rowTop is null for a line no hunk shows", () => {
  assert.equal(rowTop(file, "additions", 30, true), null);
});
