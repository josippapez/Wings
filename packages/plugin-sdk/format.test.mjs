import assert from "node:assert/strict";
import { test } from "node:test";

import { ago, basename, plural, truncate } from "./format.js";

test("ago counts minutes, hours and days, then gives the date, and is empty for a non-date", () => {
  const now = Date.parse("2026-10-13T12:00:00Z");
  assert.equal(ago("2026-10-13T12:00:00Z", now), "just now");
  assert.equal(ago("2026-10-13T11:55:00Z", now), "5 min ago");
  assert.equal(ago("2026-10-13T09:00:00Z", now), "3 h ago");
  assert.equal(ago("2026-10-11T12:00:00Z", now), "2 d ago");
  assert.match(ago("2026-01-02T12:00:00Z", now), /2026/);
  assert.equal(ago("nope", now), "");
  assert.equal(ago("2026-10-13T13:00:00Z", now), "just now");
});

test("plural counts a noun, with an irregular plural when given", () => {
  assert.equal(plural(1, "file"), "1 file");
  assert.equal(plural(0, "file"), "0 files");
  assert.equal(plural(2, "child", "children"), "2 children");
  assert.equal(plural(1, "child", "children"), "1 child");
});

test("basename is the last part of a path, and a path with no parts stays as it is", () => {
  assert.equal(basename("/Users/me/repo"), "repo");
  assert.equal(basename("/Users/me/repo/"), "repo");
  assert.equal(basename("repo"), "repo");
  assert.equal(basename("/"), "/");
});

test("truncate cuts to the length with an ellipsis, and leaves short text alone", () => {
  assert.equal(truncate("hello", 5), "hello");
  assert.equal(truncate("hello world", 5), "hell…");
});
