/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { cleanFocus, resolveFocus, type FocusFile } from "./diff-focus.ts";

const files: FocusFile[] = [
  { name: "src/a.ts", hunks: [{ additionStart: 10, additionCount: 4, deletionStart: 10, deletionCount: 3 }, { additionStart: 40, additionCount: 2, deletionStart: 39, deletionCount: 2 }] },
  { name: "src/new-name.ts", prevName: "src/old-name.ts", hunks: [{ additionStart: 1, additionCount: 5, deletionStart: 1, deletionCount: 5 }] },
];

test("finds the file by path and the line inside a hunk, on the new side by default", () => {
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts", line: 12 }), { index: 0, line: 12, side: "additions" });
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts", line: 40 }), { index: 0, line: 40, side: "additions" });
});

test("the old side counts the lines before the change", () => {
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts", line: 13, side: "old" }), { index: 0, side: "deletions" }, "13 is past the old hunk 10 to 12");
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts", line: 11, side: "old" }), { index: 0, line: 11, side: "deletions" });
});

test("a line outside every hunk, or none, only opens the file", () => {
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts", line: 25 }), { index: 0, side: "additions" });
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts", line: 14 }), { index: 0, side: "additions" }, "the hunk ends at 13");
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts" }), { index: 0, side: "additions" });
  assert.deepEqual(resolveFocus(files, { path: "src/a.ts", line: 0 }), { index: 0, side: "additions" });
});

test("a file that isn't in the patch resolves to nothing, so the viewer just opens", () => {
  assert.equal(resolveFocus(files, { path: "src/missing.ts", line: 1 }), null);
  assert.equal(resolveFocus(files, undefined), null);
  assert.equal(resolveFocus([], { path: "src/a.ts" }), null);
});

test("a renamed file matches its old or its new name", () => {
  assert.equal(resolveFocus(files, { path: "src/new-name.ts" })?.index, 1);
  assert.equal(resolveFocus(files, { path: "src/old-name.ts", line: 3, side: "old" })?.index, 1);
});

test("cleans what a plugin sends", () => {
  assert.deepEqual(cleanFocus({ path: "a.ts", line: 3, side: "old" }), { path: "a.ts", line: 3, side: "old" });
  assert.deepEqual(cleanFocus({ path: "a.ts", line: -1, side: "left", extra: 1 }), { path: "a.ts" });
  assert.deepEqual(cleanFocus({ path: "a.ts", line: 1.5 }), { path: "a.ts" });
  for (const bad of [undefined, null, "a.ts", {}, { path: "" }, { path: 3 }, { path: "x".repeat(501) }]) assert.equal(cleanFocus(bad), undefined);
});
