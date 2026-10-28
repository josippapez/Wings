/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { PluginView } from "./api.ts";
import { boundedJson, cleanNewTab, wantsNewTab, firstTabFor, newTabAction, parseNewProjectOpens, newTabEntries, openViewTab, pluginSettingsSection, restoreTabs, saveTabs, settingsPlugins, shownSection, staleViewTabs, type ViewTab } from "./plugin-pages.ts";

const plugin = (id: string, name: string, extra: Partial<PluginView> & { settings?: boolean } = {}): PluginView => {
  const { settings, ...rest } = extra;
  return {
    id,
    name,
    version: "1",
    description: "",
    main: "main.js",
    iconSvg: null,
    permissions: { exec: [], transcript: [], openUrl: [], fetch: [], post: [], panes: null, notify: false, statusline: false, ports: false, agentActivity: false, diskUsage: false, sendToAgents: false, worktrees: false, callTools: [], events: [] },
    contributes: { ui: [], mcpTools: [], panels: [], sidebars: [], views: [], ...(settings ? { settings: { page: "settings.html" } } : {}) },
    enabled: true,
    approved: true,
    source: null,
    dev: false,
    ...rest,
  };
};

test("Settings lists the plugins that are on and have a settings page, by name", () => {
  const list = [
    plugin("zed", "Zed", { settings: true }),
    plugin("plain", "Plain"),
    plugin("off", "Off", { settings: true, enabled: false }),
    plugin("unapproved", "Unapproved", { settings: true, approved: false }),
    plugin("alpha", "Alpha", { settings: true }),
  ];
  assert.deepEqual(settingsPlugins(list).map((p) => p.id), ["alpha", "zed"]);
});

test("a plugin's settings section falls back to Plugins once the plugin is off or loses its page", () => {
  const on = [plugin("timer", "Timer", { settings: true })];
  assert.equal(shownSection(pluginSettingsSection("timer"), on), "plugin:timer");
  assert.equal(shownSection(pluginSettingsSection("timer"), [plugin("timer", "Timer", { settings: true, enabled: false })]), "plugins");
  assert.equal(shownSection(pluginSettingsSection("timer"), [plugin("timer", "Timer")]), "plugins");
  assert.equal(shownSection(pluginSettingsSection("timer"), []), "plugins");
  // Wings' own sections pass through.
  assert.equal(shownSection("cursor", []), "cursor");
});

type PaneTab = { id: string; spaceId: string; layout: { kind: "pane"; id: string } };
const paneTab = (id: string, spaceId = "s1"): PaneTab => ({ id, spaceId, layout: { kind: "pane", id: `${id}-pane` } });
const board = { pluginId: "demo", viewId: "board" };

test("a view opens one tab per project, and opening it again finds that tab", () => {
  const start: (PaneTab | ViewTab)[] = [paneTab("t1")];
  const first = openViewTab(start, "s1", board, "v1");
  assert.equal(first.id, "v1");
  assert.deepEqual(first.tabs.map((t) => t.id), ["t1", "v1"]);
  const again = openViewTab(first.tabs, "s1", board, "v2");
  assert.equal(again.id, "v1");
  assert.equal(again.tabs, first.tabs);
  // Another project gets its own; another view of the same plugin too.
  assert.equal(openViewTab(first.tabs, "s2", board, "v3").id, "v3");
  assert.equal(openViewTab(first.tabs, "s1", { pluginId: "demo", viewId: "graph" }, "v4").id, "v4");
});

test("newTab always opens another tab, for a single view, an equal-args multiple view, and keeps its args", () => {
  const first = openViewTab<PaneTab>([], "s1", board, "v1");
  const second = openViewTab(first.tabs, "s1", board, "v2", { newTab: true, args: { q: 1 } });
  assert.equal(second.id, "v2");
  assert.deepEqual(second.tabs.map((t) => t.id), ["v1", "v2"]);
  assert.deepEqual((second.tabs[1] as ViewTab).args, { q: 1 });
  // The first tab was left as it was, and a plain open still finds the first one.
  assert.equal(second.tabs[0], first.tabs[0]);
  assert.equal(openViewTab(second.tabs, "s1", board, "v3").id, "v1");
  const item = { pluginId: "demo", viewId: "item" };
  const a = openViewTab<PaneTab>([], "s1", item, "i1", { multiple: true, args: { n: 1 } });
  assert.equal(openViewTab(a.tabs, "s1", item, "i2", { multiple: true, args: { n: 1 } }).id, "i1");
  const forced = openViewTab(a.tabs, "s1", item, "i2", { multiple: true, args: { n: 1 }, newTab: true });
  assert.deepEqual(forced.tabs.map((t) => t.id), ["i1", "i2"]);
});

test("a click wants a new tab with the middle button or Cmd/Ctrl", () => {
  const click = { button: 0, metaKey: false, ctrlKey: false };
  assert.equal(wantsNewTab(click), false);
  assert.equal(wantsNewTab({ ...click, button: 1 }), true);
  assert.equal(wantsNewTab({ ...click, metaKey: true }), true);
  assert.equal(wantsNewTab({ ...click, ctrlKey: true }), true);
});

test("openView's newTab is a boolean and only for a tab", () => {
  assert.equal(cleanNewTab(undefined, "modal"), false);
  assert.equal(cleanNewTab(true, "tab"), true);
  assert.equal(cleanNewTab(false, "sidebar"), false);
  assert.throws(() => cleanNewTab("yes", "tab"), /true or false/);
  assert.throws(() => cleanNewTab(true, "modal"), /as: "tab"/);
});

test("view tabs are saved apart from terminal tabs and come back at their place", () => {
  const tabs: (PaneTab | ViewTab)[] = [paneTab("t1"), { id: "v1", spaceId: "s1", view: board }, paneTab("t2"), paneTab("t3", "s2")];
  const saved = saveTabs<PaneTab>(tabs);
  assert.deepEqual(saved.tabs.map((t) => t.id), ["t1", "t2", "t3"]);
  assert.deepEqual(saved.viewTabs, [{ id: "v1", spaceId: "s1", view: board, at: 1 }]);
  const back = restoreTabs(saved.tabs, JSON.parse(JSON.stringify(saved.viewTabs)));
  assert.deepEqual(back, tabs);
  // A file from before view tabs has none; a place past the end, after a project went, goes last.
  assert.deepEqual(restoreTabs(saved.tabs).map((t) => t.id), ["t1", "t2", "t3"]);
  assert.deepEqual(restoreTabs([paneTab("t1")], [{ id: "v1", spaceId: "s1", view: board, at: 5 }]).map((t) => t.id), ["t1", "v1"]);
});

test("a view tab goes when its plugin is off or removed, or its view no longer opens as a tab", () => {
  const withBoard = (opens: ("tab" | "modal")[], extra: Partial<PluginView> = {}) => {
    const p = plugin("demo", "Demo", extra);
    p.contributes.views = [{ id: "board", title: "Board", icon: "list", page: "board.html", opens, width: null, height: null }];
    return p;
  };
  const tabs: (PaneTab | ViewTab)[] = [paneTab("t1"), { id: "v1", spaceId: "s1", view: board }];
  assert.deepEqual(staleViewTabs(tabs, [withBoard(["modal", "tab"])]), []);
  assert.deepEqual(staleViewTabs(tabs, [withBoard(["modal"])]), ["v1"]);
  assert.deepEqual(staleViewTabs(tabs, [withBoard(["tab"], { enabled: false })]), ["v1"]);
  assert.deepEqual(staleViewTabs(tabs, [withBoard(["tab"], { approved: false })]), ["v1"]);
  assert.deepEqual(staleViewTabs(tabs, []), ["v1"]);
});

test("a view that is multiple opens a new tab every time", () => {
  const first = openViewTab([paneTab("t1")], "s1", board, "v1", { multiple: true });
  const second = openViewTab(first.tabs, "s1", board, "v2", { multiple: true });
  assert.equal(second.id, "v2");
  assert.deepEqual(second.tabs.map((t) => t.id), ["t1", "v1", "v2"]);
});

test("a multiple view shows the tab that already has the same args instead of opening another", () => {
  const first = openViewTab([paneTab("t1")], "s1", board, "v1", { multiple: true, args: { pr: 7, repo: "a" } });
  const again = openViewTab(first.tabs, "s1", board, "v2", { multiple: true, args: { repo: "a", pr: 7 } });
  assert.equal(again.id, "v1");
  assert.equal(again.tabs, first.tabs);
  const other = openViewTab(first.tabs, "s1", board, "v3", { multiple: true, args: { pr: 8, repo: "a" } });
  assert.equal(other.id, "v3");
  // Another project, or no args, still opens a new tab.
  assert.equal(openViewTab(first.tabs, "s2", board, "v4", { multiple: true, args: { pr: 7, repo: "a" } }).id, "v4");
  assert.equal(openViewTab(first.tabs, "s1", board, "v5", { multiple: true }).id, "v5");
});

test("opening a view tab with different args replaces them and reloads it, same args or none leave it alone", () => {
  const opened = openViewTab([paneTab("t1")], "s1", board, "v1", { args: { task: 7 } });
  assert.deepEqual((opened.tabs[1] as ViewTab).args, { task: 7 });
  // The page saved state for item 7: same args keep it, new args drop it.
  const first = { ...opened, tabs: [opened.tabs[0], { ...(opened.tabs[1] as ViewTab), state: { scroll: 40 } }] };
  assert.equal(openViewTab(first.tabs, "s1", board, "v2", { args: { task: 7 } }).tabs, first.tabs);
  assert.equal(openViewTab(first.tabs, "s1", board, "v2", { args: { task: 7 } }).tabs, first.tabs);
  assert.equal(openViewTab(first.tabs, "s1", board, "v2").tabs, first.tabs);
  const next = openViewTab(first.tabs, "s1", board, "v2", { args: { task: 8 } });
  assert.equal(next.id, "v1");
  assert.deepEqual(next.tabs[1], { id: "v1", spaceId: "s1", view: board, args: { task: 8 }, rev: 1 });
  assert.equal("state" in (next.tabs[1] as ViewTab), false);
});

test("tab state and args are saved with the view tab and restored, without the reload count", () => {
  const tabs: (PaneTab | ViewTab)[] = [paneTab("t1"), { id: "v1", spaceId: "s1", view: board, state: { filter: "open" }, args: { task: 7 }, rev: 3 }];
  const saved = saveTabs<PaneTab>(tabs);
  assert.deepEqual(saved.viewTabs, [{ id: "v1", spaceId: "s1", view: board, state: { filter: "open" }, args: { task: 7 }, at: 1 }]);
  const back = restoreTabs(saved.tabs, JSON.parse(JSON.stringify(saved.viewTabs)));
  assert.deepEqual(back[1], { id: "v1", spaceId: "s1", view: board, state: { filter: "open" }, args: { task: 7 } });
});

test("state and args must be JSON that fits", () => {
  assert.equal(boundedJson({ a: 1 }, 100, "State"), '{"a":1}');
  assert.throws(() => boundedJson("x".repeat(200), 100, "State"), /State is limited to/);
  assert.throws(() => boundedJson(undefined, 100, "State"), /must be a JSON value/);
});

test("the + menu lists newTab views that open as a tab, from plugins that are on", () => {
  const page = (id: string, extra: Partial<PluginView["contributes"]["views"][number]> = {}) => ({ id, title: id, icon: "", page: `${id}.html`, opens: ["tab" as const], width: null, height: null, ...extra });
  const withViews = (id: string, views: ReturnType<typeof page>[], extra: Partial<PluginView> = {}) => {
    const p = plugin(id, id, extra);
    p.contributes.views = views;
    return p;
  };
  const list = [
    withViews("notes", [page("notes", { newTab: true }), page("hidden"), page("modal-only", { newTab: true, opens: ["modal"] })]),
    withViews("off", [page("x", { newTab: true })], { enabled: false }),
    withViews("tasks", [page("board", { newTab: true, multiple: true })]),
  ];
  const entries = newTabEntries(list);
  assert.deepEqual(entries.map((e) => [e.key, e.multiple]), [["notes:notes", false], ["tasks:board", true]]);
  const action = newTabAction(entries, "tasks:board");
  assert.ok(action.kind === "page" && action.entry.viewId === "board");
  assert.deepEqual(newTabAction(entries, "terminal"), { kind: "terminal" });
  assert.deepEqual(newTabAction(entries, "ask"), { kind: "ask" });
  assert.deepEqual(newTabAction(entries, "project"), { kind: "project" });
  // A chosen page that's no longer offered, nothing saved, or an unknown value falls back to a terminal.
  assert.deepEqual(newTabAction(entries, "off:x"), { kind: "terminal" });
  assert.deepEqual(newTabAction(entries, null), { kind: "terminal" });
  assert.deepEqual(newTabAction(entries, "nonsense"), { kind: "terminal" });
});

test("a project added opens its Project page only when Settings says so", () => {
  assert.equal(firstTabFor("project", true), "project");
  assert.equal(firstTabFor("terminal", true), "terminal");
  // A project that was already there, or is only being selected, keeps its terminal.
  assert.equal(firstTabFor("project", false), "terminal");
  assert.equal(parseNewProjectOpens("project"), "project");
  assert.equal(parseNewProjectOpens(undefined), "terminal");
  assert.equal(parseNewProjectOpens("nonsense"), "terminal");
});
