/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { PluginView } from "./api.ts";
import { cleanSectionRows, collapseKey, restoreCollapsed, rowName, rowTitle, rowRef, sectionGroups, toggleCollapsed, withoutPlugin, withoutProject, withoutProjectCollapsed, withSection } from "./project-sections.ts";

const views = ["board", "item"];
const row = (extra: Record<string, unknown> = {}) => ({ id: "a", label: "A", open: { view: "board" }, ...extra });

test("rows keep known fields, and bad ones are left out", () => {
  const rows = cleanSectionRows(
    [row({ detail: "d", badge: { text: "Open", tone: "success", extra: 1 }, extra: 1 }), row({ id: "" }), row({ id: "b", label: "" }), null, "x", row({ id: "c", badge: { text: "" } }), row({ id: "d", badge: { text: "t", tone: "loud" } }), row({ id: "e", icon: "pr-draft" }), row({ id: "f", icon: "rocket" })],
    views,
  );
  assert.deepEqual(rows, [
    { id: "a", label: "A", detail: "d", badge: { text: "Open", tone: "success" }, open: { view: "board" } },
    { id: "c", label: "A", open: { view: "board" } },
    { id: "d", label: "A", badge: { text: "t", tone: "neutral" }, open: { view: "board" } },
    { id: "e", label: "A", icon: "pr-draft", open: { view: "board" } },
    { id: "f", label: "A", open: { view: "board" } },
  ]);
});

test("rows are bounded: 50 rows, label and detail 120, id 64, badge 24", () => {
  const many = Array.from({ length: 80 }, (_, i) => row({ id: `r${i}` }));
  assert.equal(cleanSectionRows(many, views).length, 50);
  const [long] = cleanSectionRows([row({ label: "l".repeat(500), detail: "d".repeat(500), badge: { text: "b".repeat(500) } })], views);
  assert.deepEqual([long.label.length, long.detail?.length, long.badge?.text.length], [120, 120, 24]);
  assert.equal(cleanSectionRows([row({ id: "i".repeat(65) }), row({ id: "i".repeat(64) })], views).length, 1);
});

test("a repeated row id keeps the first row", () => {
  assert.deepEqual(cleanSectionRows([row({ label: "first" }), row({ label: "second" })], views).map((r) => r.label), ["first"]);
});

test("a row must open one of the plugin's tab views, and its args are copied and limited to 16 KB", () => {
  for (const open of [{ view: "nope" }, { view: 3 }, {}, null]) assert.throws(() => cleanSectionRows([row({ open })], views), /opens/);
  const args = { n: 1 };
  const [ok] = cleanSectionRows([row({ open: { view: "item", args } })], views);
  assert.deepEqual(ok.open, { view: "item", args: { n: 1 } });
  assert.notEqual(ok.open.args, args);
  assert.throws(() => cleanSectionRows([row({ open: { view: "item", args: "x".repeat(16 * 1024) } })], views), /16 KB/);
  assert.deepEqual(cleanSectionRows([row({ open: { view: "item", args: null } })], views)[0].open, { view: "item" });
  assert.throws(() => cleanSectionRows("rows", views), /list/);
});

test("withSection sets, replaces and removes without touching other projects", () => {
  const rows = cleanSectionRows([row()], views);
  const one = withSection({}, "p1", "tasks", "open", rows);
  const two = withSection(one, "p2", "tasks", "open", rows);
  assert.equal(two.p1, one.p1);
  assert.equal(withSection(two, "p2", "tasks", "open", cleanSectionRows([row()], views)), two, "same rows change nothing");
  assert.notEqual(withSection(two, "p2", "tasks", "open", cleanSectionRows([row({ label: "B" })], views)).p2, two.p2);
  assert.deepEqual(withSection(two, "p2", "tasks", "open", null), one);
  assert.equal(withSection(one, "p9", "tasks", "open", null), one);
  assert.deepEqual(withSection(one, "p1", "tasks", "open", []), {});
});

test("turning a plugin off clears its sections everywhere and leaves other plugins' alone", () => {
  const rows = cleanSectionRows([row()], views);
  let all = withSection({}, "p1", "tasks", "open", rows);
  all = withSection(all, "p2", "tasks", "done", rows);
  all = withSection(all, "p1", "pr-tracker", "prs", rows);
  assert.deepEqual(withoutPlugin(all, "tasks"), { p1: { "pr-tracker:prs": rows } });
  assert.deepEqual(withoutPlugin(withoutPlugin(all, "tasks"), "pr-tracker"), {});
  assert.equal(withoutPlugin(all, "other"), all);
});

const plugin = (id: string, sections: { id: string; title: string; icon: string }[], on = true) =>
  ({
    id,
    iconSvg: null,
    enabled: on,
    approved: true,
    contributes: { ui: [], mcpTools: [], panels: [], sidebars: [], views: [], projectSections: sections },
  }) as unknown as PluginView;

test("a project shows the non-empty sections of the plugins in the list, in plugin then manifest order", () => {
  const rows = cleanSectionRows([row()], views);
  const project = { "b:two": rows, "b:one": rows, "a:x": rows, "gone:x": rows };
  const plugins = [plugin("b", [{ id: "one", title: "One", icon: "" }, { id: "two", title: "Two", icon: "list" }, { id: "empty", title: "E", icon: "" }]), plugin("a", [{ id: "x", title: "X", icon: "" }])];
  assert.deepEqual(sectionGroups(project, plugins).map((g) => [g.key, g.title]), [["b:one", "One"], ["b:two", "Two"], ["a:x", "X"]]);
  assert.deepEqual(sectionGroups(undefined, plugins), []);
  // A section a plugin doesn't declare doesn't show.
  assert.deepEqual(sectionGroups({ "b:undeclared": rows }, plugins), []);
});

test("collapsed sections toggle and come back from the saved workspace, for projects that still exist", () => {
  const a = collapseKey("p1", "tasks:open");
  const b = collapseKey("p2", "tasks:open");
  let collapsed = toggleCollapsed([], a);
  collapsed = toggleCollapsed(collapsed, b);
  assert.deepEqual(collapsed, [a, b]);
  const saved = JSON.parse(JSON.stringify(collapsed));
  assert.deepEqual(restoreCollapsed(saved, ["p1", "p2"]), [a, b]);
  assert.deepEqual(restoreCollapsed(saved, ["p2"]), [b]);
  assert.deepEqual(toggleCollapsed(collapsed, a), [b]);
  assert.deepEqual(restoreCollapsed(undefined, ["p1"]), []);
  assert.deepEqual(restoreCollapsed([1, null, a], ["p1"]), [a]);
  assert.notEqual(collapseKey("p", "1:x"), collapseKey("p1", ":x"));
});

test("removing a project drops its sections and collapse keys and keeps the others", () => {
  const rows = cleanSectionRows([row()], views);
  const all = withSection(withSection({}, "p1", "tasks", "open", rows), "p2", "tasks", "open", rows);
  assert.deepEqual(withoutProject(all, "p1"), { p2: all.p2 });
  assert.equal(withoutProject(all, "p9"), all);
  const collapsed = [collapseKey("p1", "tasks:open"), collapseKey("p10", "tasks:open"), collapseKey("p2", "tasks:open")];
  assert.deepEqual(withoutProjectCollapsed(collapsed, "p1"), [collapsed[1], collapsed[2]]);
});

test("rowRef takes a leading #/! reference out of the detail", () => {
  assert.deepEqual(rowRef("#6399 · needs your review"), { ref: "#6399", rest: "needs your review" });
  assert.deepEqual(rowRef("!12"), { ref: "!12" });
  assert.deepEqual(rowRef("needs your review"), { rest: "needs your review" });
  assert.deepEqual(rowRef("Defect #185638"), { ref: "#185638", rest: "Defect" });
  assert.deepEqual(rowRef("Tech Story #187242 · Not linked to a repo"), { ref: "#187242", rest: "Tech Story · Not linked to a repo" });
  assert.deepEqual(rowRef("# not a ref"), { rest: "# not a ref" });
  assert.deepEqual(rowRef(undefined), {});
  assert.equal(rowName({ id: "1", label: "Fix it", detail: "#1 · mine", badge: { text: "Approved", tone: "success" }, open: { view: "pr" } }), "Fix it, Approved, #1 · mine");
});

test("rowTitle drops leading [tags] only", () => {
  assert.equal(rowTitle("[Waiting on decision] [#186963] FE: Spike - back navigation"), "FE: Spike - back navigation");
  assert.equal(rowTitle("[WIP] Fix it"), "Fix it");
  assert.equal(rowTitle("Fix [the] thing"), "Fix [the] thing");
  assert.equal(rowTitle("[Only] [tags]"), "[Only] [tags]");
  assert.equal(rowTitle("Plain"), "Plain");
});
