import assert from "node:assert/strict";
import { test } from "node:test";

import { withFirstPage } from "./history-rows.ts";

const row = (hash) => [hash, `subject ${hash}`, "author", 0, []];
const rows = (from, to) => Array.from({ length: to - from }, (_, i) => row(`c${from + i}`));

test("a first page after a new commit keeps the rows Load more added", () => {
  // 100 rows shown (two pages); a commit lands and the poll sends the new first 50.
  const page = [row("new"), ...rows(0, 49)];
  const next = withFirstPage(rows(0, 100), page);
  assert.equal(next.length, 101);
  assert.deepEqual(next.map(([h]) => h).slice(0, 3), ["new", "c0", "c1"]);
  assert.equal(next.at(-1)[0], "c99");
});

test("an unchanged first page leaves every row", () => {
  assert.deepEqual(withFirstPage(rows(0, 100), rows(0, 50)), rows(0, 100));
});

test("a page whose last commit isn't shown any more replaces the rows", () => {
  const rewritten = [row("x1"), row("x2")];
  assert.deepEqual(withFirstPage(rows(0, 100), rewritten), rewritten);
  assert.deepEqual(withFirstPage(rows(0, 100), []), []);
});
