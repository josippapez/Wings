/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { PluginView } from "./api.ts";
import { cleanSectionRows, collapseKey, nestRows, restoreCollapsed, rowName, rowTitle, rowRef, sectionGroups, toggleCollapsed, withoutPlugin, withoutProject, withoutProjectCollapsed, withSection, type ProjectSectionRow } from "./project-sections.ts";

const views = ["board", "item"];
const row = (extra: Record<string, unknown> = {}) => ({ id: "a", label: "A", open: { view: "board" }, ...extra });

test("rows keep known fields, and bad ones are left out", () => {
  const rows = cleanSectionRows(
    [row({ detail: "d", badge: { text: "Open", tone: "success", extra: 1 }, extra: 1 }), row({ id: "" }), row({ id: "b", label: "" }), null, "x", row({ id: "c", badge: { text: "" } }), row({ id: "d", badge: { text: "t", tone: "loud" } }), row({ id: "e", icon: "pr-draft" }), row({ id: "f", icon: "rocket" }), row({ id: "g", icon: "in-review" })],
    views,
  );
  assert.deepEqual(rows, [
    { id: "a", label: "A", detail: "d", badge: { text: "Open", tone: "success" }, open: { view: "board" } },
    { id: "c", label: "A", open: { view: "board" } },
    { id: "d", label: "A", badge: { text: "t", tone: "neutral" }, open: { view: "board" } },
    { id: "e", label: "A", icon: "pr-draft", open: { view: "board" } },
    { id: "f", label: "A", open: { view: "board" } },
    { id: "g", label: "A", icon: "in-review", open: { view: "board" } },
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

test("a row's parent is kept when it is 1 to 64 characters and not the row's own id", () => {
  const rows = cleanSectionRows(
    [row({ id: "a" }), row({ id: "b", parent: "a" }), row({ id: "c", parent: "c" }), row({ id: "d", parent: 7 }), row({ id: "e", parent: "p".repeat(65) }), row({ id: "f", parent: "p".repeat(64) }), row({ id: "g", parent: "" }), row({ id: "h", parent: "nobody" })],
    views,
  );
  const rest = { label: "A", open: { view: "board" } };
  assert.deepEqual(rows, [
    { id: "a", ...rest },
    { id: "b", parent: "a", ...rest },
    { id: "c", ...rest },
    { id: "d", ...rest },
    { id: "e", ...rest },
    { id: "f", parent: "p".repeat(64), ...rest },
    { id: "g", ...rest },
    { id: "h", parent: "nobody", ...rest },
  ]);
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

const node = (id: string, parent?: string): ProjectSectionRow => ({ id, label: id, open: { view: "board" }, ...(parent ? { parent } : {}) });
/** The rows in drawing order as `id:depth`. */
const tree = (rows: ProjectSectionRow[]) => nestRows(rows).map(({ row, depth }) => `${row.id}:${depth}`);

test("nestRows puts a row's children right after it at every depth, siblings in the order sent, and a row with an unknown parent on top", () => {
  // A child is sent before its parent, and a grandchild before its own parent.
  const rows = [node("c1", "a"), node("a"), node("x", "gone"), node("g1", "c2"), node("c2", "a"), node("b"), node("g2", "c1"), node("b1", "b")];
  assert.deepEqual(tree(rows), ["a:0", "c1:1", "g2:2", "c2:1", "g1:2", "x:0", "b:0", "b1:1"]);
  assert.deepEqual(tree([node("a"), node("b")]), ["a:0", "b:0"], "rows with no parent keep their order");
  assert.deepEqual(tree([]), []);
});

test("nestRows lists every row once when parents loop, and starts a looping row on top", () => {
  assert.deepEqual(tree([node("a", "b"), node("b", "a")]), ["a:0", "b:1"]);
  assert.deepEqual(tree([node("a", "a"), node("b")]), ["a:0", "b:0"]);
  // c hangs off the loop, d is a plain row and e is its own parent: all five show once.
  const rows = [node("c", "b"), node("d"), node("a", "b"), node("b", "a"), node("e", "e")];
  assert.deepEqual(nestRows(rows).map((n) => n.row.id).sort(), ["a", "b", "c", "d", "e"]);
});

test("nestRows draws rows deeper than 3 levels at depth 3, still in tree order", () => {
  const chain = [node("a"), node("b", "a"), node("c", "b"), node("d", "c"), node("e", "d"), node("f", "e"), node("g", "b")];
  assert.deepEqual(tree(chain), ["a:0", "b:1", "c:2", "d:3", "e:3", "f:3", "g:2"]);
});
