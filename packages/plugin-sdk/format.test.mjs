import assert from "node:assert/strict";
import { test } from "node:test";

import { ago, basename, plural, truncate } from "./format.js";

test("ago counts minutes, hours and days, then gives the date, and is empty for a non-date", () => {
  const now = Date.parse("2026-10-13T12:00:00Z");
  const at = (iso) => ago(iso, now);
  assert.deepEqual(["2026-10-13T12:00:00Z", "2026-10-13T11:55:00Z", "2026-10-13T09:00:00Z", "2026-10-11T12:00:00Z", "2026-10-13T13:00:00Z", "nope"].map(at), ["just now", "5 min ago", "3 h ago", "2 d ago", "just now", ""]);
  assert.match(at("2026-01-02T12:00:00Z"), /2026/);
});

test("plural names a count, basename takes the last part of a path, truncate cuts with an ellipsis", () => {
  assert.deepEqual([plural(1, "file"), plural(0, "file"), plural(2, "child", "children")], ["1 file", "0 files", "2 children"]);
  assert.deepEqual([basename("/Users/me/repo/"), basename("repo"), basename("/")], ["repo", "repo", "/"]);
  assert.deepEqual([truncate("hello", 5), truncate("hello world", 5)], ["hello", "hell…"]);
});
