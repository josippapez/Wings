/// <reference types="node" />
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

type Sdk = {
  setBadge(paneId: string, badge: unknown): Promise<unknown>;
  setSidebarLabel(id: string, label: string | null, extra?: { rows?: unknown[]; tone?: string }): Promise<unknown>; onBroadcast(fn: (m: unknown) => void): () => void; onPanes(fn: (panes: unknown[]) => void): () => void; onFocus(fn: (e: { focused: boolean }) => void): () => void; onProjects(fn: (p: unknown[]) => void): () => void; onGitChange(fn: (e: { projectIds: string[] }) => void): () => void; setProjectSection(projectId: string, sectionId: string, rows: unknown): Promise<unknown>; readonly focused: boolean; blockState(): Promise<unknown>; setBlockState(s: unknown): Promise<unknown>; setHeight(px: number): void; onTool(name: string, handler: (input: unknown, context: unknown) => unknown): void };

/** Loads public/plugin-sdk.js into a frame-like context, and delivers events the way the host does. */
function frame(init?: unknown) {
  const handlers: ((event: { source: unknown; data: unknown }) => void)[] = [];
  const mouse: Record<string, (event: Record<string, unknown>) => void> = {};
  const posts: { id: number; method: string; params: unknown }[] = [];
  const parent = { postMessage: (m: { id: number; method: string; params: unknown }) => posts.push(m) };
  const context: Record<string, unknown> = {
    parent,
    __wingsNonce: "n",
    __wingsInit: init,
    addEventListener: (type: string, fn: (event: { source: unknown; data: unknown }) => void) => (type === "message" ? handlers.push(fn) : void (mouse[type] = fn as never)),
    getSelection: () => null,
    console,
    queueMicrotask,
  };
  context.window = context;
  vm.createContext(context);
  vm.runInContext(readFileSync(new URL("../../public/plugin-sdk.js", import.meta.url), "utf8"), context);
  const send = (event: string, data: unknown) => handlers.forEach((fn) => fn({ source: parent, data: { wings: 1, event, data } }));
  const calls = (method: string) => posts.filter((m) => m.method === method);
  const reply = (id: number, error?: string, result: unknown = null) => handlers.forEach((fn) => fn({ source: parent, data: { wings: 1, id, result, error } }));
  return { wings: context.wings as Sdk, send, calls, reply, mouse };
}

test("onGitChange hears the project ids Wings sends, and stops once it's removed", () => {
  const { wings, send } = frame();
  const got: string[][] = [];
  const off = wings.onGitChange((e) => got.push(e.projectIds));
  send("git", { projectIds: ["a", "b"] });
  off();
  send("git", { projectIds: ["c"] });
  assert.deepEqual(got, [["a", "b"]]);
});

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

test("onProjects gets the list once at start, then on each change until it's removed, and setProjectSection sends its arguments", async () => {
  const { wings, send, calls, reply } = frame();
  const got: unknown[] = [];
  const off = wings.onProjects((p) => got.push(p));
  reply(calls("projects")[0].id, undefined, [{ id: "p" }]);
  await new Promise((r) => setTimeout(r));
  send("projects", [{ id: "q" }]);
  off();
  send("projects", [{ id: "r" }]);
  assert.deepEqual(got, [[{ id: "p" }], [{ id: "q" }]]);
  void wings.setProjectSection("p", "s", null);
  assert.deepEqual(JSON.parse(JSON.stringify(calls("setProjectSection")[0].params)), { projectId: "p", sectionId: "s", rows: null });
});

test("setProjectSection skips rows the project's section already has, and sends a change, a removal or another section", () => {
  const { wings, calls } = frame();
  const rows = [{ id: "a", label: "A" }];
  void wings.setProjectSection("/p", "s", rows);
  void wings.setProjectSection("/p", "s", [{ id: "a", label: "A" }]);
  void wings.setProjectSection("/p", "t", rows);
  void wings.setProjectSection("/q", "s", rows);
  void wings.setProjectSection("/p", "s", [{ id: "a", label: "B" }]);
  void wings.setProjectSection("/p", "s", null);
  void wings.setProjectSection("/p", "s", null);
  assert.equal(calls("setProjectSection").length, 5);
});

test("a project removed and added again gets its section sent again, and a refused call is retried", async () => {
  const { wings, send, calls, reply } = frame();
  void wings.setProjectSection("/p", "s", [{ id: "a" }]);
  void wings.setProjectSection("/q", "s", [{ id: "a" }]);
  send("projects", [{ id: "/q" }]);
  void wings.setProjectSection("/p", "s", [{ id: "a" }]);
  void wings.setProjectSection("/q", "s", [{ id: "a" }]);
  assert.deepEqual(calls("setProjectSection").map((c) => (c.params as { projectId: string }).projectId), ["/p", "/q", "/p"], "only the removed project is sent again");
  const refused = wings.setProjectSection("/r", "s", []).catch((e) => String(e.message));
  reply(calls("setProjectSection")[3].id, "no project /r");
  assert.equal(await refused, "no project /r");
  void wings.setProjectSection("/r", "s", []);
  assert.equal(calls("setProjectSection").length, 5);
});

test("in a block, blockState and setBlockState go to Wings, and setHeight sends the last height of a tick once", async () => {
  const { wings, calls } = frame({ placement: "block" });
  void wings.blockState();
  void wings.setBlockState({ a: 1 });
  assert.deepEqual([calls("blockState").length, JSON.stringify(calls("setBlockState")[0]?.params)], [1, '{"state":{"a":1}}']);
  wings.setHeight(300);
  wings.setHeight(320.4);
  await Promise.resolve();
  assert.equal(JSON.stringify(calls("setHeight").map((c) => c.params)), '[{"px":320}]');
  wings.setHeight(320);
  await Promise.resolve();
  assert.equal(calls("setHeight").length, 1);
  wings.setHeight(400);
  await Promise.resolve();
  assert.equal(JSON.stringify(calls("setHeight").map((c) => c.params)), '[{"px":320},{"px":400}]');
  assert.throws(() => wings.setHeight(NaN), /number of pixels/);
});

test("a page that mounts out of view has wings.focused false before Wings sends anything, and a page without it starts true", () => {
  assert.equal(frame({ placement: "block", focused: false }).wings.focused, false);
  assert.equal(frame({ placement: "block", focused: true }).wings.focused, true);
  assert.equal(frame({ placement: "tab" }).wings.focused, true);
  // Restored tabs and closed sidebars mount out of view, so they don't poll once at start.
  assert.equal(frame({ placement: "tab", focused: false }).wings.focused, false);
  assert.equal(frame({ placement: "sidebar", focused: false }).wings.focused, false);
  const { wings, send } = frame({ placement: "block", focused: false });
  const got: boolean[] = [];
  wings.onFocus((e) => got.push(e.focused));
  send("focus", { focused: true });
  assert.deepEqual([wings.focused, got], [true, [true]]);
});

test("outside a block, blockState is null and setBlockState and setHeight fail with a clear message", async () => {
  const { wings, calls } = frame({ placement: "tab" });
  assert.equal(await wings.blockState(), null);
  await assert.rejects(wings.setBlockState({}), /shown as a block/);
  assert.throws(() => wings.setHeight(200), /shown as a block/);
  assert.equal(calls("blockState").length + calls("setBlockState").length + calls("setHeight").length, 0);
});

test("a tool handler is told the pane Claude runs in, its project and its session, each null when there is none", async () => {
  const { wings, send } = frame();
  const contexts: unknown[] = [];
  wings.onTool("x", (_input, context) => void contexts.push(context));
  const project = { id: "s1", name: "web", path: "/work/web" };
  const session = { id: "abc", name: null };
  const none = { paneId: null, project: null, session: null };
  send("tool", { token: "t1", name: "x", arguments: {}, paneId: "p1", project, session });
  send("tool", { token: "t2", name: "x", arguments: {}, ...none });
  send("tool", { token: "t3", name: "x", arguments: {} });
  await new Promise((r) => setTimeout(r));
  assert.deepEqual(JSON.parse(JSON.stringify(contexts)), [{ paneId: "p1", project, session }, none, none]);
});

test("a real mouse back or forward press over the page asks Wings to go back or forward", () => {
  const { mouse, calls } = frame();
  const press = (type: string, button: number, isTrusted = true) => {
    const seen = { prevented: false };
    mouse[type]({ type, button, isTrusted, preventDefault: () => (seen.prevented = true), stopPropagation() {} });
    return seen.prevented;
  };
  assert.equal(press("mouseup", 3), true);
  assert.equal(press("mouseup", 4), true);
  assert.deepEqual(calls("navigate").map((c) => JSON.parse(JSON.stringify(c.params))), [{ direction: "back" }, { direction: "forward" }]);
  // The press and the click that follows are swallowed, but only the release navigates.
  assert.equal(press("mousedown", 3), true);
  assert.equal(press("auxclick", 4), true);
  assert.equal(calls("navigate").length, 2);
});

test("a script's own mouse events, and the other buttons, never navigate", () => {
  const { mouse, calls } = frame();
  mouse.mouseup({ type: "mouseup", button: 3, isTrusted: false, preventDefault() {}, stopPropagation() {} });
  for (const button of [0, 1, 2]) mouse.mouseup({ type: "mouseup", button, isTrusted: true, preventDefault() {}, stopPropagation() {} });
  assert.equal(calls("navigate").length, 0);
});
