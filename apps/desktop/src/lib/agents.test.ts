/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { Agent, AgentState } from "./api.ts";
import { elapsed, nextNeedingYou } from "./agents.ts";

const agent = (paneId: string, state: AgentState): Agent => ({
  paneId,
  spaceId: "/p",
  pid: 1,
  kind: "claude",
  sessionId: null,
  name: null,
  state,
  waitingFor: null,
  args: [],
  startedAt: 0,
  stateSince: null,
});
const list = [agent("a", "done"), agent("b", "working"), agent("c", "blocked"), agent("d", "blocked"), agent("e", "idle")];
const next = (focused: string | null, back = false) => nextNeedingYou(list, focused, back)?.paneId ?? null;

test("blocked agents come before finished ones, working and idle ones are skipped", () => {
  assert.equal(next(null), "c");
  assert.equal(next("b"), "c");
  assert.equal(next("c"), "d");
  assert.equal(next("d"), "a");
});

test("it wraps around, and goes the other way with back", () => {
  assert.equal(next("a"), "c");
  assert.equal(next("c", true), "a");
  assert.equal(next("a", true), "d");
  assert.equal(next(null, true), "a");
});

test("nothing needs you", () => {
  assert.equal(nextNeedingYou([agent("b", "working")], null), null);
  assert.equal(nextNeedingYou([], "x"), null);
});

test("an agent that needs you alone is where you already are", () => {
  assert.equal(nextNeedingYou([agent("c", "blocked")], "c")?.paneId, "c");
});

test("elapsed time reads as seconds, minutes, then hours", () => {
  assert.deepEqual([0, 42_000, 60_000, 7 * 60_000 + 5_000, 65 * 60_000, -5].map(elapsed), ["0s", "42s", "1m", "7m", "1h 5m", "0s"]);
});
