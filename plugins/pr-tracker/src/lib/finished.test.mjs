import assert from "node:assert/strict";
import test from "node:test";
import { eventFor, finalState, retry, settle } from "./finished.ts";

const where = { projectId: "p1", cwd: "/repo" };
const item = (n, head = `feature/${n}`) => ({ link: { provider: "github", url: `https://github.com/acme/web/pull/${n}`, number: n, repo: "acme/web" }, title: `PR ${n}`, head });

test("a repo listed for the first time has nothing gone", () => {
  const { next, gone } = settle(undefined, [item(1)], where);
  assert.equal(gone.length, 0);
  assert.deepEqual([...next.keys()], ["https://github.com/acme/web/pull/1"]);
});

test("a PR missing from the new list is gone, with its source branch", () => {
  const { next } = settle(undefined, [item(1), item(2)], where);
  const second = settle(next, [item(2)], where);
  assert.deepEqual(second.gone.map((t) => t.head), ["feature/1"]);
  assert.equal(eventFor(second.gone[0], "merged").sourceBranch, "feature/1");
  assert.deepEqual(eventFor(second.gone[0], "merged"), { url: "https://github.com/acme/web/pull/1", state: "merged", sourceBranch: "feature/1", repo: "acme/web", provider: "github", number: 1, projectId: "p1" });
});

test("finalState reads merged and closed, and says nothing for an open PR", () => {
  assert.equal(finalState("pr-merged"), "merged");
  assert.equal(finalState("pr-closed"), "closed");
  assert.equal(finalState("pr-open"), null);
});

test("a failed lookup is kept for the next check, three tries in all", () => {
  const { next } = settle(undefined, [item(1)], where);
  let tracked = [...next.values()][0];
  for (let i = 0; i < 2; i++) {
    const after = new Map();
    retry(after, tracked);
    assert.equal(after.size, 1);
    tracked = [...after.values()][0];
  }
  const last = new Map();
  retry(last, tracked);
  assert.equal(last.size, 0);
});
