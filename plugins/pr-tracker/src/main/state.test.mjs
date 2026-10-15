import assert from "node:assert/strict";
import test from "node:test";
import { diffs, failures, links, prune, status, viewers } from "./state.ts";

const keep = (url) => {
  status.set(url, { error: "x" });
  diffs.set(url, { key: "k", patch: "", comments: [] });
  failures.set(url, { key: "k", items: Promise.resolve([]), list: [] });
};
const kept = (url) => [status.has(url), diffs.has(url), failures.has(url)];

test("a PR's status, diff and failed checks go once no pane, list or open diff viewer shows it", (t) => {
  t.after(() => [status, diffs, failures, viewers, links].forEach((m) => m.clear()));
  for (const url of ["on-pane", "listed", "in-viewer", "gone"]) keep(url);
  links.set("p1", { provider: "github", url: "on-pane", number: 1, repo: "o/r" });
  viewers.set("diff-1", "in-viewer");
  prune(["listed"]);
  assert.deepEqual(["on-pane", "listed", "in-viewer"].map(kept), [[true, true, true], [true, true, true], [true, true, true]]);
  assert.deepEqual(kept("gone"), [false, false, false]);
  prune([]);
  assert.deepEqual(kept("listed"), [false, false, false]);
  assert.deepEqual(kept("on-pane"), [true, true, true]);
});

test("only the newest diff viewers are kept track of, since Wings doesn't say when one is closed", (t) => {
  t.after(() => [status, diffs, failures, viewers, links].forEach((m) => m.clear()));
  for (let i = 0; i < 25; i++) viewers.set(`diff-${i}`, `pr-${i}`);
  prune([]);
  assert.equal(viewers.size, 20);
  assert.equal(viewers.has("diff-4"), false);
  assert.equal(viewers.has("diff-5"), true);
  assert.equal(viewers.has("diff-24"), true);
});
