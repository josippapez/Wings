import assert from "node:assert/strict";
import test from "node:test";
import { BLOCK_ROWS, countPulls, filterOf, filterPulls, fitPulls, orderPulls, splitTitle } from "./pulls.ts";

const row = (id, scope, draft = false) => ({ id, scope, draft });
const rows = [row("a", "other"), row("b", "mine"), row("c", "review", true), row("d", "other", true), row("e", "mine")];

test("orderPulls puts yours first, then the ones asked of you, then the rest, each in the list's order", () => {
  assert.deepEqual(orderPulls(rows).map((r) => r.id), ["b", "e", "c", "a", "d"]);
  assert.deepEqual(orderPulls([]), []);
});

test("each filter shows its own rows, and All open shows every one", () => {
  assert.deepEqual(filterPulls(rows, "all").map((r) => r.id), ["a", "b", "c", "d", "e"]);
  assert.deepEqual(filterPulls(rows, "mine").map((r) => r.id), ["b", "e"]);
  assert.deepEqual(filterPulls(rows, "review").map((r) => r.id), ["c"]);
  assert.deepEqual(filterPulls(rows, "drafts").map((r) => r.id), ["c", "d"]);
  assert.deepEqual(countPulls(rows), { all: 5, mine: 2, review: 1, drafts: 2 });
  assert.deepEqual(countPulls([]), { all: 0, mine: 0, review: 0, drafts: 0 });
});

test("a saved filter is used only when it is one of the filters", () => {
  assert.equal(filterOf({ filter: "mine" }), "mine");
  assert.equal(filterOf({ filter: "drafts" }), "drafts");
  assert.equal(filterOf({ filter: "everything" }), "all");
  assert.equal(filterOf(null), "all");
  assert.equal(filterOf("mine"), "all");
});

test("leading [tags] of a title are split off, and a title that is only tags stays whole", () => {
  assert.deepEqual(splitTitle("[ABC-12] [web] Add filters"), { tags: ["ABC-12", "web"], text: "Add filters" });
  assert.deepEqual(splitTitle("Fix [typo] in docs"), { tags: [], text: "Fix [typo] in docs" });
  assert.deepEqual(splitTitle("[WIP]"), { tags: [], text: "[WIP]" });
  assert.deepEqual(splitTitle("  plain  "), { tags: [], text: "plain" });
});

test("a list too big for a broadcast is cut at the last row that fits, a normal one is kept whole", () => {
  const args = { provider: "azure", url: "https://dev.azure.com/org/Some%20Project/_git/some-repo/pullrequest/12345", number: 12345, repo: "Some Project/some-repo", title: "x".repeat(120), cwd: "/Users/someone/projects/some-repo", remote: { kind: "azure", org: "https://dev.azure.com/org", project: "Some Project", repo: "some-repo" } };
  const one = { id: "azure:12345", title: "x".repeat(160), number: "#12345", author: "A Person With A Long Name", branch: `feature/${"y".repeat(60)} → main`, scope: "other", draft: false, badge: { text: "Checks failing", tone: "danger" }, review: "1 approved, 1 waiting for author", checks: { passed: 4, failed: 1, running: 2 }, updated: "2026-10-13T10:00:00.000Z", unread: 3, agent: "working", args };
  const long = fitPulls(Array.from({ length: BLOCK_ROWS }, () => one));
  assert.ok(long.length > 40 && long.length < BLOCK_ROWS);
  // Wings refuses a broadcast over 64,000 characters, counted with the envelope the messages module puts around an answer.
  assert.ok(JSON.stringify({ "wings:sdk": 1, kind: "answer", id: `${Math.random().toString(36).slice(2)}:123456`, response: { rows: long } }).length <= 64_000);
  assert.equal(fitPulls(Array.from({ length: 50 }, () => ({ ...one, title: "short", args: { ...args, title: "short" } }))).length, 50);
  assert.deepEqual(fitPulls([]), []);
});
