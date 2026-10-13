/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { PluginView } from "./api.ts";
import { openViewTab, pluginSettingsSection, restoreTabs, saveTabs, settingsPlugins, shownSection, staleViewTabs, type ViewTab } from "./plugin-pages.ts";

const plugin = (id: string, name: string, extra: Partial<PluginView> & { settings?: boolean } = {}): PluginView => {
  const { settings, ...rest } = extra;
  return {
    id,
    name,
    version: "1",
    description: "",
    main: "main.js",
    iconSvg: null,
    permissions: { exec: [], transcript: [], openUrl: [], fetch: [], post: [], panes: null, notify: false, statusline: false, ports: false },
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
