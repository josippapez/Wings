import assert from "node:assert/strict";
import { test } from "node:test";

import { mergeState } from "./state-merge.ts";

const repo = { root: "/work/web", name: "web", branch: "main" };
const files = [["M", "src/a.ts", 1, 0, null]];
const good = { type: "state", mode: "uncommitted", repo, files, total: 1, added: 1, removed: 0 };

test("an error for the same repo and mode keeps the list and carries the error", () => {
  const next = mergeState(good, { type: "state", mode: "uncommitted", repo: { root: "/work/web", name: "web" }, error: "index.lock exists" });
  assert.equal(next.files, files);
  assert.equal(next.error, "index.lock exists");
  assert.equal(next.loading, false);
});

test("a good state after the error clears it", () => {
  const withError = mergeState(good, { type: "state", mode: "uncommitted", repo, error: "boom" });
  const next = mergeState(withError, { ...good, total: 2 });
  assert.equal(next.error, undefined);
  assert.equal(next.total, 2);
});

test("a refresh of the same repo and mode keeps the list", () => {
  const next = mergeState(good, { type: "state", mode: "uncommitted", repo, loading: true });
  assert.equal(next.files, files);
  assert.equal(next.loading, true);
});

test("another repo or mode replaces the list, error or not", () => {
  const other = { root: "/work/api", name: "api" };
  assert.equal(mergeState(good, { type: "state", mode: "uncommitted", repo: other, error: "x" }).files, undefined);
  assert.equal(mergeState(good, { type: "state", mode: "branch", repo, error: "x" }).files, undefined);
  assert.equal(mergeState(good, { type: "state", mode: "uncommitted", repo: null }).repo, null);
});

test("an error with no list to keep is shown alone", () => {
  const next = mergeState(null, { type: "state", mode: "uncommitted", repo, error: "x" });
  assert.equal(next.error, "x");
  assert.equal(next.files, undefined);
});
