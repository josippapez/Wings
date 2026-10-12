/// <reference types="node" />
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

type Sdk = { onBroadcast(fn: (m: unknown) => void): () => void; onPanes(fn: (panes: unknown[]) => void): () => void };

/** Loads public/plugin-sdk.js into a frame-like context, and delivers events the way the host does. */
function frame() {
  const handlers: ((event: { source: unknown; data: unknown }) => void)[] = [];
  const parent = { postMessage() {} };
  const context: Record<string, unknown> = {
    parent,
    __wingsNonce: "n",
    addEventListener: (type: string, fn: (event: { source: unknown; data: unknown }) => void) => type === "message" && handlers.push(fn),
    getSelection: () => null,
    console,
  };
  context.window = context;
  vm.createContext(context);
  vm.runInContext(readFileSync(new URL("../../public/plugin-sdk.js", import.meta.url), "utf8"), context);
  const send = (event: string, data: unknown) => handlers.forEach((fn) => fn({ source: parent, data: { wings: 1, event, data } }));
  return { wings: context.wings as Sdk, send };
}

test("an on* listener stops getting events once it's removed", () => {
  const { wings, send } = frame();
  const got: unknown[] = [];
  const off = wings.onBroadcast((m) => got.push(m));
  send("broadcast", 1);
  off();
  send("broadcast", 2);
  assert.deepEqual(got, [1]);
});

test("a listener that removes itself doesn't make the next one miss the event", () => {
  const { wings, send } = frame();
  const got: string[] = [];
  const off = wings.onPanes(() => {
    got.push("first");
    off();
  });
  wings.onPanes(() => got.push("second"));
  send("panes", []);
  send("panes", []);
  assert.deepEqual(got, ["first", "second", "second"]);
});
