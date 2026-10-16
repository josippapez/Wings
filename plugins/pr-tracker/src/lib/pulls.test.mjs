import assert from "node:assert/strict";
import test from "node:test";
import { countPulls, filterOf, filterPulls, orderPulls, splitTitle } from "./pulls.ts";

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
