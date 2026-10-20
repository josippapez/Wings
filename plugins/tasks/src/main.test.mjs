// main.ts end to end: Claude's tools and a page's operations, on a wings with its storage in memory and a relay that carries
// broadcasts between the main frame and one page frame.
import test from "node:test";
import assert from "node:assert/strict";
import { channel } from "wings-plugin-sdk/messages";

const storage = new Map();
const handlers = new Map();
const sections = [];
const listeners = { main: new Set(), page: new Set() };
const send = (to, message) => queueMicrotask(() => listeners[to].forEach((listener) => listener(JSON.parse(JSON.stringify(message)))));
const settle = () => new Promise((resolve) => setImmediate(resolve));
let projects;
let panes;
const hooks = new Map();
const rails = [];

globalThis.wings = {
  storage: {
    get: async (key) => storage.get(key) ?? null,
    set: async (key, value) => void storage.set(key, JSON.parse(JSON.stringify(value))),
    delete: async (key) => void storage.delete(key),
    entries: async (prefix) => [...storage].filter(([k]) => k.startsWith(prefix)).sort(([a], [b]) => (a < b ? -1 : 1)),
  },
  broadcast: async (message) => send("page", message),
  onBroadcast: (listener) => (listeners.main.add(listener), () => listeners.main.delete(listener)),
  onTool: (name, handler) => handlers.set(name, handler),
  onProjects: (listener) => (projects = listener),
  onPanes: (listener) => (panes = listener),
  onHook: (event, handler) => hooks.set(event, handler),
  setPaneRail: async (paneId, rail) => void rails.push({ paneId, rail }),
  setProjectSection: async (id, section, rows) => void sections.push({ id, section, rows }),
};
await import("./main.ts");
const { slotOf } = await import("./lib/storage.ts");

const page = channel({ wings: { broadcast: async (message) => send("main", message), onBroadcast: (listener) => (listeners.page.add(listener), () => listeners.page.delete(listener)) } });
const told = [];
page.on("issues", (m) => told.push(m));
const todoTold = [];
page.on("todo", (m) => todoTold.push(m));
const call = async (name, input) => handlers.get(name)(input, { paneId: "p1", project: { id: "proj1", name: "web", path: "/work/web" }, session: { id: "s-1", name: "agent one" } });
const ask = (op) => page.ask("op", { projectId: "proj1", projectName: "web", op });

test("a tool's input is checked against its schema first, and the error names the field and what it allows", async () => {
  const wrong = [
    ["get", {}, /key is required/],
    ["get", { key: "" }, /key needs at least 1 character/],
    ["get", { key: "A", extra: 1 }, /Unknown field: extra/],
    ["get", { key: "W".repeat(21) }, /key is limited to 20 characters/],
    ["get", null, /must be an object/],
    ["create", { title: "" }, /title needs at least 1 character/],
    ["create", { title: 5 }, /title must be text/],
    ["create", { title: "x".repeat(201) }, /title is limited to 200 characters, this one has 201/],
    ["create", { title: "ok", description: "x".repeat(20_001) }, /description is limited to 20,000/],
    ["create", { title: "t", status: "wip" }, /status must be one of todo, in_progress, in_review, done, canceled/],
    ["create", { title: "t", priority: "asap" }, /priority must be one of urgent, high, medium, low, none/],
    ["create", { title: "t", type: "story" }, /type must be one of task, bug, feature, epic/],
    ["update", { key: "WEB-1" }, /Pass at least one of title, description, .*, blocked_by_add or blocked_by_remove\./],
    ["update", { key: "WEB-1", related_add: "WEB-2" }, /related_add must be a list/],
    ["update", { key: "WEB-1", blocked_by_add: [3] }, /blocked_by_add\[0\] must be text/],
    ["update", { key: "WEB-1", related_add: Array(21).fill("WEB-2") }, /related_add takes up to 20 items, this one has 21/],
    ["update", { key: "WEB-1", parent: 4 }, /parent must be text/],
    ["update", { key: "WEB-1", commit: "a".repeat(41) }, /commit is limited to 40 characters/],
    ["log", { key: "WEB-1", text: "x".repeat(20_001) }, /text is limited to 20,000 characters/],
    ["list", { status: "x" }, /status must be one of/],
    ["list", { type: "x" }, /type must be one of/],
    ["list", { query: 5 }, /query must be text/],
    ["list", { query: "x".repeat(201) }, /query is limited to 200 characters/],
  ];
  for (const [name, input, message] of wrong) await assert.rejects(call(name, input), message, `${name} ${JSON.stringify(input)}`);
  assert.equal(storage.size, 0, "nothing was written");
});

test("tools write as the Claude session in the calling pane, and say what they did", async () => {
  assert.match(await call("create", { title: "Do it", description: "Goal", status: "in_progress" }), /^Created WEB-1 in web\./);
  assert.equal(await call("log", { key: "web-1", text: "next: tests" }), "Logged on WEB-1.");
  const got = await call("get", { key: " web-1 " });
  for (const part of ["WEB-1 [in_progress] (task) Do it", "Goal", "s-1", "agent one", "agent one note: next: tests"]) assert.ok(got.includes(part), part);
  assert.equal(await call("update", { key: "WEB-1", status: "done" }), "Updated WEB-1: Do it [done].");
  assert.equal(await call("update", { key: "WEB-1", status: "done" }), "WEB-1 already has that. Nothing changed.");
  assert.match(await call("list", { query: "  do " }), /^Issues for web matching "do"\./);
  await assert.rejects(call("log", { key: "WEB-1", text: "  " }), /text is required/);
  await assert.rejects(call("get", { key: "web-9" }), /No issue WEB-9 in web/);
  await settle();
  assert.equal(told.length, 3, "a change tells the pages, a change of nothing doesn't");
});

test("a page's operation is answered once it is saved: pages are told once, a change of nothing tells none, an error reaches the page", async () => {
  const before = told.length;
  await ask({ kind: "update", key: "WEB-1", title: "Done it" });
  await settle();
  assert.deepEqual(told.slice(before), [{ projectId: "proj1" }]);
  assert.equal(storage.get(`issues/${slotOf("proj1")}/WEB-1`).title, "Done it");
  await ask({ kind: "update", key: "WEB-1", title: "Done it" });
  await settle();
  assert.equal(told.length, before + 1);
  await assert.rejects(ask({ kind: "update", key: "WEB-1", status: "later" }), /status must be one of/);
  assert.equal(told.length, before + 1);
});

test("the project's section shows its open issues after a change, and is filled when Wings lists the projects", async () => {
  await ask({ kind: "create", title: "Second" });
  const rows = [{ id: "WEB-2", label: "WEB-2 Second", icon: "todo", open: { view: "issue", args: { key: "WEB-2", projectId: "proj1" } } }];
  assert.deepEqual(sections.at(-1), { id: "proj1", section: "tasks", rows });
  projects([{ id: "proj1", name: "web" }, { id: "proj2", name: "api" }]);
  await settle();
  assert.deepEqual(sections.filter((s) => s.id === "proj1").at(-1).rows, rows);
  assert.equal(sections.filter((s) => s.id === "proj2").at(-1).rows, null, "a project with no issues has no section");
});

const ctx = { paneId: "p1", project: { id: "proj1", name: "web", path: "/work/web" }, session: { id: "s-1", name: "agent one" } };

test("the todo tool keeps the session's list, links it to the issue, tells the pages and shows the card on the session's pane", async () => {
  await panes([{ paneId: "p1", projectId: "proj1", cwd: "/work/web", command: "claude", project: "web", session: { sessionId: "s-1", name: "agent one", state: "idle", startedAt: 1 }, focused: true }]);
  await settle();
  assert.equal(rails.at(-1).rail, null, "no list, no card");
  assert.match(await call("todo", {}), /No todo list/);
  const out = await call("todo", { items: [{ text: "read", status: "done" }, { text: "write", status: "in_progress" }], key: "web-1" });
  assert.match(out, /\(WEB-1\): 1 of 2 done\.\n\[x\] read\n\[~\] write/);
  await settle();
  assert.deepEqual(todoTold, [{ projectId: "proj1" }]);
  assert.deepEqual(rails.at(-1), { paneId: "p1", rail: { title: "Todo · WEB-1", items: [{ text: "read", status: "done" }, { text: "write", status: "in_progress" }], open: "todo" } });
  assert.equal(JSON.parse(JSON.stringify([...storage].find(([k]) => k.startsWith("todo/"))[1])).versions.length, 1);
  await call("todo", { items: [{ text: "read", status: "done" }, { text: "write", status: "done" }] });
  assert.equal(rails.at(-1).rail, null, "all done clears the card");
  await assert.rejects(handlers.get("todo")({}, { ...ctx, session: null }), /no agent session/);
  await assert.rejects(call("todo", { items: [{ text: "x", status: "pending" }], key: "WEB-99" }), /No issue WEB-99/);
});

test("the session start line and the commit check answer from storage", async () => {
  const start = await hooks.get("SessionStart")({ session_id: "s-1" }, ctx);
  assert.match(start, /Wings Issues is the local tracker for web/);
  assert.match(start, /This session is on WEB-\d "/);
  assert.equal(await hooks.get("SessionStart")({ session_id: "s-1" }, { ...ctx, project: null }), null);
  const bash = (command) => hooks.get("PreToolUse")({ session_id: "s-1", tool_name: "Bash", tool_input: { command } }, ctx);
  assert.match((await bash('git commit -m "WEB-1 done"')).deny, /^WEB-1 is a local Wings issue/);
  assert.equal(await bash('git commit -m "done"'), null);
  await wings.storage.set("keepLocal", false);
  assert.equal(await bash('git commit -m "WEB-1 done"'), null, "with the setting off nothing is denied");
});
