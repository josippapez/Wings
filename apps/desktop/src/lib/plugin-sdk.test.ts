/// <reference types="node" />
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

type Sdk = {
  setBadge(paneId: string, badge: unknown): Promise<unknown>;
  setSidebarLabel(id: string, label: string | null, extra?: { rows?: unknown[]; tone?: string }): Promise<unknown>; onBroadcast(fn: (m: unknown) => void): () => void; onPanes(fn: (panes: unknown[]) => void): () => void; onFocus(fn: (e: { focused: boolean }) => void): () => void; readonly focused: boolean };

/** Loads public/plugin-sdk.js into a frame-like context, and delivers events the way the host does. */
function frame() {
  const handlers: ((event: { source: unknown; data: unknown }) => void)[] = [];
  const posts: { id: number; method: string; params: unknown }[] = [];
  const parent = { postMessage: (m: { id: number; method: string; params: unknown }) => posts.push(m) };
  const context: Record<string, unknown> = {
    parent,
    __wingsNonce: "n",
    addEventListener: (type: string, fn: (event: { source: unknown; data: unknown }) => void) => type === "message" && handlers.push(fn),
    getSelection: () => null,
    console,
    queueMicrotask,
  };
  context.window = context;
  vm.createContext(context);
  vm.runInContext(readFileSync(new URL("../../public/plugin-sdk.js", import.meta.url), "utf8"), context);
  const send = (event: string, data: unknown) => handlers.forEach((fn) => fn({ source: parent, data: { wings: 1, event, data } }));
  const calls = (method: string) => posts.filter((m) => m.method === method);
  const reply = (id: number, error?: string) => handlers.forEach((fn) => fn({ source: parent, data: { wings: 1, id, result: null, error } }));
  return { wings: context.wings as Sdk, send, calls, reply };
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

test("setBadge skips a badge the pane already has, and sends a changed one or a removal", () => {
  const { wings, calls } = frame();
  void wings.setBadge("p1", { text: "a" });
  void wings.setBadge("p1", { text: "a" });
  void wings.setBadge("p2", { text: "a" });
  void wings.setBadge("p1", { text: "b" });
  void wings.setBadge("p1", null);
  void wings.setBadge("p1", null);
  assert.equal(calls("setBadge").length, 4);
});

test("a skipped setBadge still resolves", async () => {
  const { wings } = frame();
  void wings.setBadge("p1", { text: "a" });
  assert.equal(await wings.setBadge("p1", { text: "a" }), null);
});

test("a badge is sent again after its pane left the list, or after Wings refused it", async () => {
  const { wings, send, calls, reply } = frame();
  void wings.setBadge("p1", { text: "a" });
  send("panes", [{ paneId: "p1" }]);
  void wings.setBadge("p1", { text: "a" });
  assert.equal(calls("setBadge").length, 1);
  send("panes", []);
  void wings.setBadge("p1", { text: "a" });
  assert.equal(calls("setBadge").length, 2);
  const rejected = wings.setBadge("p2", { text: "x" }).catch((e) => String(e.message));
  reply(calls("setBadge")[2].id, "no pane p2");
  assert.equal(await rejected, "no pane p2");
  void wings.setBadge("p2", { text: "x" });
  assert.equal(calls("setBadge").length, 4);
});

test("setSidebarLabel skips an identical label and sends when text, rows or tone change", () => {
  const { wings, calls } = frame();
  const rows = [{ label: "a", value: "1" }];
  void wings.setSidebarLabel("s", "5h 40%", { rows, tone: "warning" });
  void wings.setSidebarLabel("s", "5h 40%", { rows: [{ label: "a", value: "1" }], tone: "warning" });
  void wings.setSidebarLabel("s", "5h 40%", { rows: [{ label: "a", value: "2" }], tone: "warning" });
  void wings.setSidebarLabel("s", "5h 40%", { rows: [{ label: "a", value: "2" }] });
  void wings.setSidebarLabel("t", "5h 40%");
  void wings.setSidebarLabel("t", null);
  void wings.setSidebarLabel("t", null);
  assert.equal(calls("setSidebarLabel").length, 5);
});

test("onFocus gets the state Wings sent, then each change, and stops once removed", () => {
  const { wings, send } = frame();
  const got: boolean[] = [];
  const off = wings.onFocus((e) => got.push(e.focused));
  assert.equal(wings.focused, true);
  send("focus", { focused: true });
  send("focus", { focused: false });
  assert.equal(wings.focused, false);
  send("focus", { focused: false });
  send("focus", { focused: true });
  off();
  send("focus", { focused: false });
  assert.deepEqual(got, [true, false, true]);
});

test("onFocus registered after the state is known hears it right away", async () => {
  const { wings, send } = frame();
  send("focus", { focused: false });
  const got: boolean[] = [];
  wings.onFocus((e) => got.push(e.focused));
  assert.deepEqual(got, []);
  await Promise.resolve();
  assert.deepEqual(got, [false]);
});
