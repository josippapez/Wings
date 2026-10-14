import assert from "node:assert/strict";
import { test } from "node:test";
import { advance, enqueue } from "./review-queue.ts";

test("every plugin waiting for review is kept, in order, one entry each", () => {
  let q = enqueue([], { id: "a", v: 1 }, { id: "b", v: 1 });
  q = enqueue(q, { id: "c", v: 1 }, { id: "a", v: 2 });
  assert.deepEqual(q, [{ id: "a", v: 2 }, { id: "b", v: 1 }, { id: "c", v: 1 }]);
  q = advance(q);
  assert.deepEqual(q.map((p) => p.id), ["b", "c"]);
  assert.deepEqual(advance(advance(q)), []);
});
