/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { tabCloser } from "./close-tabs.ts";
import { pane, paneIds, split, type LayoutNode } from "./layout.ts";

/** State as React keeps it: changes wait for the next render, and the ref handlers read follows only then. */
function reactState<T>(initial: T) {
  const queue: ((value: T) => T)[] = [];
  const state = { value: initial, ref: { current: initial } };
  const set = (next: T | ((value: T) => T)) => void queue.push(typeof next === "function" ? (next as (value: T) => T) : () => next);
  const render = () => {
    state.value = queue.splice(0).reduce((value, change) => change(value), state.value);
    state.ref.current = state.value;
  };
  return { state, set, render };
}

type Tab = { id: string; spaceId: string; layout: LayoutNode; focusedPane: string; zoomedPane: string | null };
const tab = (id: string, spaceId: string, layout: LayoutNode): Tab => ({ id, spaceId, layout, focusedPane: paneIds(layout)[0], zoomedPane: null });

test("panes exiting back to back, before React renders again, each close from what the one before left", () => {
  // The terminal host died: every pane it ran exits at once, in no particular order.
  for (const order of [["a1", "b1", "a2"], ["b1", "a2", "a1"], ["a1", "a2", "b1"]]) {
    const tabs = reactState([tab("a", "one", split(pane("a1"), "a1", "column", "a2")), tab("x", "one", pane("x1")), tab("b", "two", pane("b1")), tab("y", "two", pane("y1"))]);
    const active = reactState<Record<string, string>>({ one: "a", two: "b" });
    const disposed: string[] = [];
    const { closePane } = tabCloser(tabs.state.ref, tabs.set, active.set, (key) => disposed.push(key));
    for (const key of order) closePane(key);
    tabs.render();
    active.render();
    assert.deepEqual(tabs.state.value.map((t) => t.id), ["x", "y"], `closing ${order}`);
    assert.deepEqual(active.state.value, { one: "x", two: "y" }, `closing ${order}`);
    assert.deepEqual(disposed.sort(), ["a1", "a2", "b1"], `closing ${order}`);
  }
});
