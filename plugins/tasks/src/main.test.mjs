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
const sectionSorts = [];
let onSort;

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
  projects: async () => [],
  onPanes: (listener) => (panes = listener),
  onHook: (event, handler) => hooks.set(event, handler),
  setPaneRail: async (paneId, rail) => void rails.push({ paneId, rail }),
  setProjectSection: async (id, section, rows) => void sections.push({ id, section, rows }),
  setSectionSort: async (section, sort) => void sectionSorts.push({ section, sort }),
  onSectionSort: (listener) => (onSort = listener),
  focused: false,
  onFocus: () => () => {},
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

test("the project's section lists children after their epic or feature and names it as their parent, unless it isn't open", async () => {
  const docs = (op) => page.ask("op", { projectId: "proj3", projectName: "docs", op });
  const keys = () => sections.filter((s) => s.id === "proj3").at(-1).rows.map((r) => r.id);
  const parents = () => Object.fromEntries(sections.filter((s) => s.id === "proj3").at(-1).rows.filter((r) => "parent" in r).map((r) => [r.id, r.parent]));
  for (const issue of [{ title: "Plain" }, { title: "Epic", type: "epic" }, { title: "Feature", type: "feature", parent: "DOC-2" }, { title: "Started", status: "in_progress", parent: "DOC-2" }, { title: "Lone", status: "in_progress" }, { title: "Deep", parent: "DOC-3" }]) {
    await docs({ kind: "create", ...issue });
  }
  // Open issues run in progress first, then by key (4, 5, 1, 2, 3, 6); each one's children follow it, and theirs follow them.
  assert.deepEqual(keys(), ["DOC-5", "DOC-1", "DOC-2", "DOC-4", "DOC-3", "DOC-6"]);
  assert.deepEqual(parents(), { "DOC-4": "DOC-2", "DOC-3": "DOC-2", "DOC-6": "DOC-3" });
  // With the epic done, its children are top-level rows again, with no parent to name.
  await docs({ kind: "update", key: "DOC-2", status: "done" });
  assert.deepEqual(keys(), ["DOC-4", "DOC-5", "DOC-1", "DOC-3", "DOC-6"]);
  assert.deepEqual(parents(), { "DOC-6": "DOC-3" });
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

test("the section puts the issue an agent works on now first, sorts again when its pane goes, and follows the Sidebar order", async () => {
  const app = { id: "proj5", name: "app", path: "/work/app" };
  const as = (session) => (name, input) => handlers.get(name)(input, { paneId: "p5", project: app, session });
  await as(null)("create", { title: "First" });
  await as({ id: "s-9", name: "agent nine" })("create", { title: "Second" });
  const keys = () => sections.filter((s) => s.id === "proj5").at(-1).rows.map((r) => r.id);
  const pane = { paneId: "p5", projectId: "proj5", cwd: "/work/app", command: "claude", project: "app", session: { sessionId: "s-9", name: "agent nine", state: "working", startedAt: 1 }, focused: false };
  await panes([pane]);
  await settle();
  assert.deepEqual(keys(), ["APP-2", "APP-1"]);
  await panes([{ ...pane, session: null }]);
  await settle();
  assert.deepEqual(keys(), ["APP-1", "APP-2"]);
  // Two edits in one millisecond would tie on latest change.
  await new Promise((resolve) => setTimeout(resolve, 2));
  await as(null)("update", { key: "APP-1", title: "First, renamed" });
  assert.deepEqual(keys(), ["APP-1", "APP-2"], "by status, the older key stays first");
  const { sectionOrder } = await import("./lib/section-setting.ts");
  await sectionOrder.set("date");
  await settle();
  assert.deepEqual(keys(), ["APP-1", "APP-2"]);
  await new Promise((resolve) => setTimeout(resolve, 2));
  await as(null)("update", { key: "APP-2", title: "Second, renamed" });
  assert.deepEqual(keys(), ["APP-2", "APP-1"], "by latest change, the newest edit is first");
  await sectionOrder.set("status");
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

test("the journal links a turn's commits and fills Files changed on this session's open issue without writing log notes, and does nothing else", async () => {
  const sess = { id: "j-1", name: "journal agent", kind: "opencode" };
  const jctx = { ...ctx, session: sess };
  const journal = hooks.get("Journal");
  const readIssues = async () => (await import("./lib/storage.ts")).readStore("proj1", "web").then((s) => s.issues);
  const open = (await call("create", { title: "Journaled", status: "in_progress" })).match(/WEB-\d+/)[0];
  const issue = async (key) => (await readIssues()).find((i) => i.key === key);
  // Put the session on the issue, as a tool call from it would.
  await handlers.get("log")({ key: open, text: "started" }, jctx);
  const logBefore = (await issue(open)).log.length;

  const payload = { session_id: "j-1", cwd: "/work/web", agent: "opencode", prompt: "do it", files: [{ path: "src/a.ts", what: "adds the X type" }, { path: "b.md", what: "" }], more: 2, commits: ["abc123def456", "1234567"] };
  assert.equal(await journal({ hook_event_name: "Journal", ...payload }, jctx), null, "it answers with nothing");
  let now = await issue(open);
  assert.deepEqual(now.links.commits, ["abc123d", "1234567"]);
  assert.equal(now.log.length, logBefore + 2, "only the two commit links, no note for the files");
  assert.equal(now.log.filter((e) => e.kind === "note").length, 1);
  assert.deepEqual(now.files.map((f) => [f.path, f.what, f.session]), [["src/a.ts", "adds the X type", sess], ["b.md", "", sess]]);

  // A later turn: the same path keeps one row with the newest description, and the same commits add nothing.
  await journal({ hook_event_name: "Journal", ...payload, files: [{ path: "src/a.ts", what: "renames X to Y" }] }, jctx);
  now = await issue(open);
  assert.equal(now.links.commits.length, 2);
  assert.equal(now.log.length, logBefore + 2);
  assert.deepEqual(now.files.map((f) => [f.path, f.what]).sort(), [["b.md", ""], ["src/a.ts", "renames X to Y"]]);
  assert.match(await call("get", { key: open }), /Files changed \(2\):\n {2}src\/a\.ts — renames X to Y \(journal agent, /);

  // No files: nothing written for them. An unknown session, or an issue that is done, is left alone.
  await journal({ hook_event_name: "Journal", ...payload, files: [], more: 0 }, jctx);
  const before = JSON.stringify(await readIssues());
  await journal({ hook_event_name: "Journal", ...payload, session_id: "nobody", commits: ["fedcba9"] }, { ...jctx, session: { id: "nobody", name: null, kind: "claude" } });
  assert.equal(JSON.stringify(await readIssues()), before);
  await handlers.get("update")({ key: open, status: "done" }, jctx);
  const done = JSON.stringify(await readIssues());
  await journal({ hook_event_name: "Journal", ...payload, commits: ["fedcba9"] }, jctx);
  assert.equal(JSON.stringify(await readIssues()), done);
  assert.equal(await journal({ hook_event_name: "Journal", ...payload }, { ...jctx, project: null }), null);
});

test("an order picked in the section header's menu is saved as the setting and confirmed to Wings", async () => {
  await settle();
  assert.deepEqual(sectionSorts.at(0), { section: "tasks", sort: "status" }, "the saved order is shown on start");
  onSort({ sectionId: "tasks", sort: "date" });
  await settle();
  assert.equal(storage.get("sectionOrder"), "date");
  assert.deepEqual(sectionSorts.at(-1), { section: "tasks", sort: "date" });
  onSort({ sectionId: "tasks", sort: "bogus" });
  await settle();
  assert.equal(storage.get("sectionOrder"), "date", "an order it doesn't know changes nothing");
});

test("each message in a session whose issue is in review asks the agent to settle it; other statuses and sessions get nothing", async () => {
  const sess = { id: "r-1", name: "review agent", kind: "claude" };
  const rctx = { ...ctx, session: sess };
  const say = (session_id) => hooks.get("UserPromptSubmit")({ session_id, prompt: "it works, done" }, { ...rctx, session: { ...sess, id: session_id } });
  const made = await handlers.get("create")({ title: "Wait for the user" }, rctx);
  const key = made.match(/Created (WEB-\d+)/)[1];
  assert.equal(await say("r-1"), null, "to do");
  await handlers.get("update")({ key, status: "in_review" }, rctx);
  assert.match(await say("r-1"), new RegExp(`^This session's ${key} "Wait for the user" is In review\\. If this message says the work is done`));
  assert.equal(await say("someone-else"), null);
  await handlers.get("update")({ key, status: "done" }, rctx);
  assert.equal(await say("r-1"), null, "done");
});
