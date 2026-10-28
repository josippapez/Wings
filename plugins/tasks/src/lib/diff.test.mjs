import assert from "node:assert/strict";
import { test } from "node:test";

import { diffArgs, lineCounts } from "./diff.ts";

test("a row reads its line counts from the journal's text, and anything else stays text", () => {
  assert.deepEqual(lineCounts("changed (+15 -6)"), { added: 15, removed: 6 });
  assert.equal(lineCounts("renames X to Y"), null);
});

test("a file's diff starts before the issue's first commit, or at HEAD without one", () => {
  assert.deepEqual(diffArgs({ links: { commits: ["abc1234", "def5678"] } }, "src/a.ts"), ["diff", "abc1234^", "--no-color", "--no-ext-diff", "--", "src/a.ts"]);
  assert.deepEqual(diffArgs({}, "src/a.ts"), ["diff", "HEAD", "--no-color", "--no-ext-diff", "--", "src/a.ts"]);
});
