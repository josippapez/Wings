import test from "node:test";
import assert from "node:assert/strict";
import { applyOp } from "./ops.ts";
import { matchesQuery } from "./search.ts";
import { create, fresh } from "./testing.mjs";

test("search: every word, any case, over key, title, description and log notes", () => {
  let s = fresh();
  s = create(s, "Checkout summary", { description: "Round the Totals once", type: "bug" }).store;
  s = create(s, "Login page", { status: "in_progress" }).store;
  s = applyOp(s, { kind: "note", key: "WEB-2", text: "Waiting on the Checkout API" }, "you", 3).store;
  const [a, b] = s.issues;
  assert.ok(matchesQuery(a, "") && matchesQuery(a, "   ") && matchesQuery(a, undefined), "no words matches all");
  assert.ok(matchesQuery(a, "CHECKOUT"), "title, any case");
  assert.ok(matchesQuery(a, "round totals"), "description, words in any order");
  assert.ok(matchesQuery(a, "totals round"));
  assert.ok(!matchesQuery(a, "totals login"), "every word has to match");
  assert.ok(matchesQuery(b, "checkout api"), "log note");
  assert.ok(matchesQuery(b, "web-2") && matchesQuery(a, "web-1"), "key");
  assert.ok(!matchesQuery(b, "zebra"));
  // The text is cached per issue, so a changed issue (a new object) is read again.
  assert.ok(!matchesQuery({ ...a, description: "" }, "totals"));
});
