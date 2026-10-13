import assert from "node:assert/strict";
import { test } from "node:test";

import { batched, fitPatch, OUTPUT_TOO_LARGE } from "./batch.ts";

const big = new Error(OUTPUT_TOO_LARGE);
const file = (name, body = "x") => `diff --git a/${name} b/${name}\nindex 1..2 100644\n--- a/${name}\n+++ b/${name}\n@@ -1 +1 @@\n-a\n+${body}\n`;

test("batched makes one call per batch and keeps the order", async () => {
  const calls = [];
  const out = await batched([1, 2, 3, 4, 5], async (p) => (calls.push(p), p.join(",") + ";"), String, { size: 2, parallel: 1 });
  assert.deepEqual(calls, [[1, 2], [3, 4], [5]]);
  assert.equal(out, "1,2;3,4;5;");
});

test("batched keeps the order when batches finish out of turn", async () => {
  const out = await batched([1, 2, 3, 4], (p) => new Promise((r) => setTimeout(() => r(p.join("")), p[0] === 1 ? 20 : 0)), String, { size: 1, parallel: 4 });
  assert.equal(out, "1234");
});

test("a batch that is too large is split in half until it fits", async () => {
  const calls = [];
  const run = async (p) => {
    calls.push(p.length);
    if (p.length > 2) throw big;
    return p.join("");
  };
  assert.equal(await batched(["a", "b", "c", "d", "e"], run, String, { size: 5 }), "abcde");
  assert.deepEqual(calls, [5, 3, 2, 1, 2]);
});

test("one file that is too large on its own becomes a placeholder and the rest stays", async () => {
  const run = async (p) => {
    if (p.includes("huge")) throw big;
    return p.join("");
  };
  assert.equal(await batched(["a", "huge", "b"], run, (f) => `[${f} too large]`, { size: 3 }), "a[huge too large]b");
});

test("other errors are not retried", async () => {
  await assert.rejects(batched(["a", "b"], async () => Promise.reject(new Error("git diff failed")), String), /git diff failed/);
});

test("fitPatch turns a file over the limit into a Too large entry and keeps the others", () => {
  const out = fitPatch(file("a.ts") + file("bundle.js", "y".repeat(3000)) + file("c.ts"), { file: 1000, total: 1e9 });
  assert.match(out, /diff --git a\/bundle\.js b\/bundle\.js\nindex 1\.\.2 100644\n--- a\/bundle\.js\n\+\+\+ b\/bundle\.js\n@@ -1 \+1 @@\n Too large to show \(0\.0 MB\)\n/);
  assert.ok(out.includes(file("a.ts")) && out.includes(file("c.ts")));
  assert.ok(!out.includes("yyyy"));
});

test("fitPatch shrinks the biggest files until the total fits", () => {
  const out = fitPatch(file("s.ts") + file("m.ts", "m".repeat(500)) + file("l.ts", "l".repeat(900)), { file: 1e9, total: 1000 });
  assert.ok(out.includes("Too large to show") && !out.includes("llll"));
  assert.ok(out.includes("mmmm"), "the smaller file stays once the total fits");
});
