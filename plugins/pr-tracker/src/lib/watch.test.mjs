import assert from "node:assert/strict";
import test from "node:test";
import { MAX_SINGLE, newPulls, notesFor, watchOf } from "./watch.ts";

const pr = (url, mine = false) => ({ link: { url }, mine });

test("a repo's first list is where counting starts, so nothing in it is new", () => {
  assert.deepEqual(newPulls(undefined, [pr("a"), pr("b")]), []);
});

test("a PR that wasn't open on the last check is new, unless you wrote it", () => {
  const before = new Set(["a"]);
  assert.deepEqual(newPulls(before, [pr("a"), pr("b"), pr("c", true)]).map((i) => i.link.url), ["b"]);
  assert.deepEqual(newPulls(new Set(), [pr("a")]).map((i) => i.link.url), ["a"]);
});

test("each new PR gets its own notification, and a burst gets one", () => {
  assert.deepEqual(notesFor("web", [{ title: "Fix login", author: "Ana" }, { title: "Docs", author: null }]), [
    { title: "New pull request in web", body: "Fix login · Ana" },
    { title: "New pull request in web", body: "Docs" },
  ]);
  const burst = Array.from({ length: MAX_SINGLE + 1 }, (_, n) => ({ title: `PR ${n}`, author: null }));
  assert.deepEqual(notesFor("web", burst), [{ title: `${MAX_SINGLE + 1} new pull requests in web`, body: burst.map((i) => i.title).join("\n") }]);
});

test("a saved interval that isn't one of the choices is off", () => {
  assert.equal(watchOf(10), 10);
  assert.equal(watchOf(0), 0);
  assert.equal(watchOf(7), 0);
  assert.equal(watchOf("10"), 0);
  assert.equal(watchOf(null), 0);
});
