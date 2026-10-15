import assert from "node:assert/strict";
import { test } from "node:test";

import { layout } from "./graph-layout.ts";

const commit = (hash, ...parents) => ({ hash, parents });

test("layout gives a branch a lane of its own until the merge, and leaves its input alone", () => {
  const commits = [commit("m", "a", "b"), commit("a", "r"), commit("b", "r"), commit("r")];
  const { rows, lanes } = layout(commits);
  assert.deepEqual(rows.map((r) => r.lane), [0, 0, 1, 0]);
  assert.equal(lanes, 2);
  assert.equal(rows[2].color, 1, "the branch has a colour of its own");
  assert.equal(commits[0].lane, undefined);
});

test("a straight history stays in one lane", () => {
  const { rows, lanes } = layout([commit("c", "b"), commit("b", "a"), commit("a")]);
  assert.deepEqual(rows.map((r) => r.lane), [0, 0, 0]);
  assert.equal(lanes, 1);
});
