import assert from "node:assert/strict";
import test from "node:test";
import { MAX_SINGLE, newPulls, newReviews, notesFor, notifyPulls, sidebarPulls, watchMinutes, watchOf } from "./watch.ts";

const pr = (url, mine = false, reviewing = false) => ({ link: { url }, mine, reviewing });

test("a repo's first list is where counting starts, so nothing in it is new", () => {
  assert.deepEqual(newPulls(undefined, [pr("a"), pr("b")]), []);
});

test("a PR that wasn't open on the last check is new, unless you wrote it", () => {
  const before = new Set(["a"]);
  assert.deepEqual(newPulls(before, [pr("a"), pr("b"), pr("c", true)]).map((i) => i.link.url), ["b"]);
  assert.deepEqual(newPulls(new Set(), [pr("a")]).map((i) => i.link.url), ["a"]);
});

test("a PR asks for your review when it's new with you on it or you're added to an open one, never on the first list", () => {
  assert.deepEqual(newReviews(undefined, [pr("a", false, true)]), []);
  const before = new Set(["a"]);
  const items = [pr("a", false, true), pr("b", false, true), pr("c", false, false), pr("d", true, true)];
  assert.deepEqual(newReviews(before, items).map((i) => i.link.url), ["b"]);
  // `before` only holds the ones that asked last time, so an open PR you're added to now counts.
  assert.deepEqual(newReviews(new Set(), [pr("c", false, true)]).map((i) => i.link.url), ["c"]);
});

test("a review request reads as one, alone and in a burst", () => {
  const item = (n) => ({ title: `PR ${n}`, author: "Ana", url: `https://host/pr/${n}`, args: { n } });
  assert.equal(notesFor("web", [item(1)], true)[0].title, "Review asked in web");
  assert.equal(notesFor("web", [1, 2, 3, 4].map(item), true)[0].title, "4 pull requests to review in web");
});

test("each new PR gets its own notification with its buttons, and a burst gets one", () => {
  const all = { label: "All pull requests", open: { view: "pr" } };
  const item = (title, author, n) => ({ title, author, url: `https://host/pr/${n}`, args: { n } });
  assert.deepEqual(notesFor("web", [item("Fix login", "Ana", 1), item("Docs", null, 2)]), [
    { title: "New pull request in web", body: "Fix login · Ana", actions: [{ label: "Open", open: { view: "pr", args: { n: 1 } } }, { label: "Open in browser", url: "https://host/pr/1" }, all] },
    { title: "New pull request in web", body: "Docs", actions: [{ label: "Open", open: { view: "pr", args: { n: 2 } } }, { label: "Open in browser", url: "https://host/pr/2" }, all] },
  ]);
  const burst = Array.from({ length: MAX_SINGLE + 1 }, (_, n) => item(`PR ${n}`, null, n));
  assert.deepEqual(notesFor("web", burst), [{ title: `${MAX_SINGLE + 1} new pull requests in web`, body: burst.map((i) => i.title).join("\n"), actions: [all] }]);
});

test("a saved interval that isn't one of the choices is not accepted", () => {
  assert.equal(watchOf(10), 10);
  assert.equal(watchOf(0), undefined);
  assert.equal(watchOf(7), undefined);
  assert.equal(watchOf("10"), undefined);
  assert.equal(watchOf(null), undefined);
});

test("the background check is every 5 minutes until a valid interval is saved, and a change reaches the main script", async (t) => {
  const stored = new Map();
  globalThis.wings = { storage: { get: async (key) => stored.get(key) ?? null, set: async (key, value) => void stored.set(key, value) }, broadcast: async () => {}, onBroadcast: () => () => {} };
  t.after(() => delete globalThis.wings);
  assert.equal(await watchMinutes.get(), 5);
  stored.set("watch-minutes", 0);
  assert.equal(await watchMinutes.get(), 5);
  assert.equal(await notifyPulls.get(), "off");
  assert.equal(await sidebarPulls.get(), "mine");
  stored.set("notify-pulls", "review");
  stored.set("sidebar-pulls", "every");
  assert.equal(await notifyPulls.get(), "review");
  assert.equal(await sidebarPulls.get(), "mine");
  stored.set("watch-minutes", 15);
  assert.equal(await watchMinutes.get(), 15);
  const heard = [];
  watchMinutes.onChange((minutes) => heard.push(minutes));
  await watchMinutes.set(30);
  assert.deepEqual(heard, [30]);
  assert.equal(stored.get("watch-minutes"), 30, "saved under the key Settings always used");
});
