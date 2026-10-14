/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { canGoBack, canGoForward, EMPTY_NAV, goBack, goForward, NAV_LIMIT, visit, type NavState, type Place } from "./nav-history.ts";

const at = (tab: string, space = "p"): Place => ({ space, tab });
const all = () => true;
const walk = (...places: Place[]): NavState => places.reduce(visit, EMPTY_NAV);
const tabs = (stack: Place[]) => stack.map((p) => p.tab);

test("the first place is where you are, with nothing behind it", () => {
  const s = walk(at("a"));
  assert.deepEqual(s.current, at("a"));
  assert.deepEqual(s.back, []);
  assert.equal(canGoBack(s, all), false);
});

test("going somewhere puts where you were on the back stack", () => {
  const s = walk(at("a"), at("b"), at("c"));
  assert.deepEqual(tabs(s.back), ["a", "b"]);
  assert.equal(s.current?.tab, "c");
});

test("showing the same place again, and showing no place, change nothing", () => {
  const s = walk(at("a"), at("b"));
  assert.equal(visit(s, at("b")), s);
  assert.equal(visit(s, null), s);
});

test("the same tab id in another project is another place", () => {
  assert.deepEqual(tabs(walk(at("a", "p"), at("a", "q")).back), ["a"]);
});

test("the back stack keeps the latest places only", () => {
  const s = walk(...Array.from({ length: NAV_LIMIT + 20 }, (_, i) => at(`t${i}`)));
  assert.equal(s.back.length, NAV_LIMIT);
  assert.equal(s.back[0].tab, "t19");
  assert.equal(s.back[NAV_LIMIT - 1].tab, `t${NAV_LIMIT + 18}`);
});

test("back and forward move between places without adding any", () => {
  let s = walk(at("a"), at("b"), at("c"));
  s = goBack(s, all);
  assert.equal(s.current?.tab, "b");
  assert.deepEqual(tabs(s.back), ["a"]);
  assert.deepEqual(tabs(s.forward), ["c"]);
  // The app shows the place it was sent to, which visits it.
  assert.equal(visit(s, at("b")), s);
  s = goForward(s, all);
  assert.equal(s.current?.tab, "c");
  assert.deepEqual(tabs(s.back), ["a", "b"]);
  assert.deepEqual(s.forward, []);
});

test("back all the way, then forward all the way, returns to the start", () => {
  const start = walk(at("a"), at("b"), at("c"), at("d"));
  let s = start;
  for (let i = 0; i < 3; i++) s = goBack(s, all);
  assert.equal(s.current?.tab, "a");
  assert.equal(goBack(s, all), s);
  for (let i = 0; i < 3; i++) s = goForward(s, all);
  assert.deepEqual(s, start);
  assert.equal(goForward(s, all), s);
});

test("going somewhere new after going back clears forward", () => {
  let s = goBack(walk(at("a"), at("b"), at("c")), all);
  assert.equal(canGoForward(s, all), true);
  s = visit(s, at("x"));
  assert.deepEqual(s.forward, []);
  assert.equal(canGoForward(s, all), false);
  assert.deepEqual(tabs(s.back), ["a", "b"]);
});

test("back skips places whose tab or project is gone, and drops them", () => {
  const s = walk(at("a"), at("b"), at("c", "gone"), at("d"));
  const exists = (p: Place) => p.space !== "gone" && p.tab !== "b";
  assert.equal(canGoBack(s, exists), true);
  const back = goBack(s, exists);
  assert.equal(back.current?.tab, "a");
  assert.deepEqual(back.back, []);
  assert.deepEqual(tabs(back.forward), ["d"]);
});

test("forward skips places that are gone too", () => {
  const s = goBack(goBack(walk(at("a"), at("b"), at("c")), all), all);
  const exists = (p: Place) => p.tab !== "b";
  assert.equal(goForward(s, exists).current?.tab, "c");
});

test("with every place behind you gone there is nowhere to go back to", () => {
  const s = walk(at("a"), at("b"));
  const none = (p: Place) => p.tab === "b";
  assert.equal(canGoBack(s, none), false);
  assert.equal(goBack(s, none), s);
});

test("a place equal to where you are is not a place to go back to", () => {
  // Closing the tab you were on lands on a tab that is already on the back stack.
  const s = walk(at("a"), at("b"), at("a"));
  assert.equal(goBack(s, all).current?.tab, "b");
});
