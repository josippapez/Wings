import assert from "node:assert/strict";
import test from "node:test";
import { dropClosed } from "./view.ts";

const ref = (id) => ({ org: "https://dev.azure.com/o", id, key: `https://dev.azure.com/o ${id}` });

test("a closed pane's ref goes, and so does an item no ref points at, so nothing polls it", () => {
  const a = ref("1234");
  const b = ref("5678");
  const refs = new Map([["p1", a], ["p2", b], ["p3", a]]);
  const items = new Map([[a.key, { title: "A" }], [b.key, { title: "B" }]]);
  dropClosed(refs, items, ["p2", "p3"]);
  assert.deepEqual([...refs.keys()], ["p2", "p3"]);
  assert.deepEqual([...items.keys()], [a.key, b.key], "p3 still points at A");
  dropClosed(refs, items, ["p2"]);
  assert.deepEqual([...refs.keys()], ["p2"]);
  assert.deepEqual([...items.keys()], [b.key]);
  dropClosed(refs, items, []);
  assert.equal(refs.size + items.size, 0);
});
