// node --test: what the native components render, as the JSON tree the app draws.
import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement as h, Fragment, useEffect, useState } from "react";

const trees = [];
globalThis.wings = { render: (json) => trees.push(JSON.parse(json)) };
const { Avatar, Svg, Markdown, ProjectGate, Skeleton, Skeletons, Summary, SummaryChip, Tabs, ListDetail, CodeBlock, useViewArgs, Badge, Button, Card, Checkbox, Chip, Chips, Combobox, Confirm, Disclosure, Dot, Empty, Field, Gauge, Graph, Heading, Icon, List, ListButton, ListItem, Loading, Menu, Notice, Page, Properties, Row, SearchField, Select, Spinner, Stack, Text, TextInput, Textarea, Time, Title, Toolbar, Bar, Scroll, Split, Sunburst, Timeline, TimelineCard, TimelineNote, mount, useAction, useTabState } = await import("./native.js");

const find = (node, type) => (node.type === type ? node : node.children?.map((child) => find(child, type)).find(Boolean));
const all = (node, type) => [...(node.type === type ? [node] : []), ...(node.children ?? []).flatMap((child) => all(child, type))];
const text = (node) => (node.type === "#text" ? node.text : (node.children ?? []).map(text).join(""));
const last = () => trees[trees.length - 1];

test("Changes controls deliver search, chip and row actions without DOM objects", () => {
  let input, chip, clicked = false;
  mount(h(Stack, null,
    h(SearchField, { label: "Search commits", defaultValue: "coupon", onInput: (e) => { input = e.currentTarget.value; } }),
    h(Chips, { label: "Show", options: [{ value: "history", label: "History" }], value: "history", onChange: (v) => { chip = v; } }),
    h(ListButton, { onClick: () => { clicked = true; } }, "Commit")));
  assert.equal(find(last(), "SearchField").props.defaultValue, "coupon");
  globalThis.__wingsEvent(find(last(), "SearchField").props.onInput, '{"value":"shipping"}');
  globalThis.__wingsEvent(find(last(), "Chips").props.onChange, '{"value":"history"}');
  globalThis.__wingsEvent(find(last(), "ListButton").props.onClick, '');
  assert.equal(input, "shipping"); assert.equal(chip, "history"); assert.equal(clicked, true);
});

test("a Stack tells the native app its maxWidth", () => {
  mount(h(Stack, { maxWidth: 720, gap: "l" }, h(Text, null, "Form")));
  assert.deepEqual(find(last(), "Stack").props, { gap: "l", maxWidth: 720 });
});

test("a 10,000 commit graph transports one node and only its visible row window", () => {
  let range, selected;
  const rows = Array.from({ length: 64 }, (_, i) => ({ hash: String(i), subject: "Commit", author: "Ada", date: "2026-11-06", refs: [], lane: 0, color: 0, merge: false, lines: [0, 0, 2, 0] }));
  mount(h(Graph, { total: 10000, lanes: 1, selected: -1, offset: 5000, rows, onRangeChange: (v) => { range = v; }, onSelect: (i) => { selected = i; } }));
  const graph = find(last(), "Graph");
  assert.equal(graph.children.length, 0); assert.equal(graph.props.rows.length, 64);
  assert.ok(JSON.stringify(last()).length < 16000);
  globalThis.__wingsEvent(graph.props.onRangeChange, '{"start":7000,"end":7064}');
  globalThis.__wingsEvent(graph.props.onSelect, '{"index":7010}');
  assert.equal(range.start, 7000); assert.equal(range.end, 7064); assert.equal(selected, 7010);
});

function Counter() {
  const [n, setN] = useState(0);
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  return h(
    Page,
    { toolbar: h(Toolbar, { title: "Pilot", actions: h(Badge, { tone: "info" }, mounted ? "ready" : "starting") }) },
    h(Stack, { gap: "l" }, h(Text, { tone: "muted" }, `Count ${n}`), h(Button, { variant: "outline", onClick: () => setN(n + 1) }, "Add")),
  );
}

test("mount renders the first tree before it returns, and a state set by an effect follows on a later turn", async () => {
  mount(h(Counter));
  assert.equal(text(find(last(), "Badge")), "starting");
  await new Promise((resolve) => setTimeout(resolve, 20));
  const tree = last();
  assert.equal(tree.type, "#root");
  const page = tree.children[0];
  assert.equal(page.type, "Page");
  assert.deepEqual(page.children.map((slot) => [slot.type, slot.props.name]), [["Slot", "toolbar"], ["Slot", "body"]]);
  assert.equal(text(find(tree, "Text")), "Count 0");
  assert.deepEqual(find(tree, "Badge").props, { tone: "info" });
  assert.equal(text(find(tree, "Badge")), "ready");
});

test("a function prop is a stable id, and firing it renders the new tree before the call returns", () => {
  const button = find(last(), "Button");
  const id = button.props.onClick;
  assert.match(id, /^\d+:onClick$/);
  assert.equal(button.props.variant, "outline");
  assert.equal(globalThis.__wingsEvent(id, ""), true);
  assert.equal(text(find(last(), "Text")), "Count 1");
  assert.equal(find(last(), "Button").props.onClick, id);
  assert.equal(globalThis.__wingsEvent(id, '{"x":1}'), true);
  assert.equal(text(find(last(), "Text")), "Count 2");
});

test("an event for a handler that isn't on the page does nothing", () => {
  const before = trees.length;
  assert.equal(globalThis.__wingsEvent("999999:onClick", ""), false);
  assert.equal(globalThis.__wingsEvent("garbage", ""), false);
  assert.equal(trees.length, before);
});

test("slots only appear for what was given, and props outside the documented ones stay out", () => {
  mount(h(Page, null, h(Empty, { title: "Nothing", className: "x" }), h(Loading), h(Row, { gap: "s", style: { color: "red" } }, "a")));
  const tree = last();
  const empty = find(tree, "Empty");
  assert.deepEqual(empty.children.map((slot) => slot.props.name), ["title"]);
  assert.deepEqual(empty.props, {});
  assert.deepEqual(find(tree, "Row").props, { gap: "s" });
  assert.equal(find(tree, "Loading").children.length, 0);
  assert.equal(tree.children[0].children.length, 1, "no toolbar slot without a toolbar");
});

test("a gauge, a disclosure and the list parts send their props and slots, and drop what ui has and they don't", () => {
  mount(
    h(
      Page,
      null,
      h(Card, { compact: true, className: "x" }, h(Heading, null, "Usage"), h(Gauge, { label: "Memory", percent: 42, levels: [75, 90], note: "5 of 12 GB", tone: "warn" }), h(Gauge, { label: "Empty note", percent: 1, note: "" })),
      h(List, null, h(ListItem, { title: "hover" }, h(Dot, { tone: "danger" }), h(Text, null, "one"))),
      h(Disclosure, { summary: "2 need sign-in", defaultOpen: true }, h(Text, null, "inside")),
    ),
  );
  const tree = last();
  assert.deepEqual(find(tree, "Card").props, {});
  assert.deepEqual(find(tree, "Dot").props, { tone: "danger" });
  assert.deepEqual(find(tree, "ListItem").props, {});
  const [gauge, bare] = all(tree, "Gauge");
  assert.deepEqual(gauge.props, { label: "Memory", percent: 42, levels: [75, 90] });
  assert.deepEqual(gauge.children.map((slot) => [slot.props.name, text(slot)]), [["note", "5 of 12 GB"]]);
  assert.equal(bare.children.length, 0, "an empty note is no slot, as ui draws none");
  const disclosure = find(tree, "Disclosure");
  assert.deepEqual(disclosure.props, {});
  assert.deepEqual(disclosure.children.map((slot) => [slot.props.name, text(slot)]), [["summary", "2 need sign-in"], ["body", "inside"]]);
});

test("a select's handler gets the value the app sends, not the event", () => {
  const picked = [];
  const options = [{ value: "a", label: "A" }, { value: "b", label: "B", disabled: true }];
  mount(h(Page, null, h(Select, { label: "Session", options, value: "a", onChange: (value) => picked.push(value) })));
  const select = find(last(), "Select");
  assert.deepEqual({ ...select.props, onChange: undefined }, { label: "Session", options, value: "a", onChange: undefined });
  assert.match(select.props.onChange, /^\d+:onChange$/);
  assert.equal(globalThis.__wingsEvent(select.props.onChange, '{"value":"b"}'), true);
  assert.deepEqual(picked, ["b"]);
  mount(h(Page, null, h(Select, { options })));
  assert.equal("onChange" in find(last(), "Select").props, false, "no handler, no id");
});

test("checkboxes send their next state and confirmation actions keep their error", async () => {
  const checked = [];
  mount(h(Page, null, h(Checkbox, { checked: false, onChange: (next) => checked.push(next) }, "Delete branch"), h(Spinner, { label: "Refreshing" }), h(Confirm, { title: "Remove?", actions: [{ id: "remove", label: "Remove", onConfirm: () => { throw new Error("blocked"); } }], onCancel: () => {} }, h(Notice, { tone: "warn" }, "Careful"))));
  const checkbox = find(last(), "Checkbox");
  assert.equal(globalThis.__wingsEvent(checkbox.props.onChange, '{"checked":true}'), true);
  assert.deepEqual(checked, [true]);
  assert.deepEqual(find(last(), "Spinner").props, { label: "Refreshing" });
  const confirm = find(last(), "Confirm");
  assert.equal(text(confirm), "Remove?CarefulRemoveCancel");
  const remove = all(last(), "Button").find((button) => text(button) === "Remove");
  assert.equal(globalThis.__wingsEvent(remove.props.onClick, ""), true);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(text(find(last(), "Notice")), "blocked");
});

test("confirmation cancel never runs the destructive action, and its action is disabled while busy", async () => {
  let calls = 0;
  let release;
  mount(h(Page, null, h(Confirm, { title: "Remove?", actions: [{ id: "remove", label: "Remove", onConfirm: () => new Promise((resolve) => { calls++; release = resolve; }) }], onCancel: () => calls += 100 })));
  const buttons = () => all(last(), "Button");
  const cancel = buttons().find((button) => text(button) === "Cancel");
  assert.equal(globalThis.__wingsEvent(cancel.props.onClick, ""), true);
  assert.equal(calls, 100);
  const remove = buttons().find((button) => text(button) === "Remove");
  assert.equal(globalThis.__wingsEvent(remove.props.onClick, ""), true);
  assert.equal(calls, 101);
  assert.ok(buttons().every((button) => button.props.disabled === true));
  release();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.ok(buttons().every((button) => button.props.disabled !== true));
});

test("a form's inputs take the field's label, hold the page's text and give its handler a change event with target.value", () => {
  function Form() {
    const [name, setName] = useState("Morning");
    const [prompt, setPrompt] = useState("");
    return h(Stack, null,
      h(Field, { label: "Name", hint: "Optional." }, h(TextInput, { value: name, maxLength: 80, onChange: (e) => setName(e.target.value) })),
      h(Field, { label: "Prompt", error: "Needed." }, h(Textarea, { value: prompt, rows: 4, onChange: (e) => setPrompt(e.currentTarget.value) })),
      h(Text, null, `${name}|${prompt}`));
  }
  mount(h(Form));
  const input = find(last(), "TextInput");
  assert.deepEqual({ ...input.props, onChange: undefined }, { value: "Morning", maxLength: 80, label: "Name", onChange: undefined });
  const area = find(last(), "Textarea");
  assert.equal(area.props.rows, 4);
  assert.equal(area.props.label, "Prompt");
  assert.equal(globalThis.__wingsEvent(input.props.onChange, '{"value":"Evening"}'), true);
  assert.equal(globalThis.__wingsEvent(find(last(), "Textarea").props.onChange, '{"value":"Check CI"}'), true);
  assert.equal(text(all(last(), "Text").at(-1)), "Evening|Check CI");
  assert.match(text(last()), /Optional\./);
  assert.match(text(last()), /Needed\./);
});

test("a combobox hands autoFocus to its field", () => {
  mount(h(Combobox, { label: "Task", value: "", options: [], onPick: () => {}, autoFocus: true }));
  assert.equal(find(last(), "TextInput").props.autoFocus, true);
});

test("a password input says so, and other input types are plain text", () => {
  mount(h(Stack, null, h(TextInput, { label: "API token", type: "password", value: "secret" }), h(TextInput, { label: "Start", type: "time", value: "09:00" })));
  const [token, start] = all(last(), "TextInput");
  assert.equal(token.props.type, "password");
  assert.equal("type" in start.props, false);
});

test("a busy button ignores clicks and shows a spinner, an icon button is named by its aria-label, and the small parts render as text", () => {
  mount(h(Page, null,
    h(Button, { busy: true, onClick: () => {} }, "Start"),
    h(Button, { "aria-label": "Delete x", size: "icon-sm" }, h(Icon, { name: "trash" })),
    h(Properties, { items: [{ label: "Project", value: "wings" }] }),
    h(Time, { iso: new Date(Date.now() - 3 * 3_600_000).toISOString(), prefix: "Checked " })));
  const [busy, icon] = all(last(), "Button");
  assert.equal(busy.props.disabled, true);
  assert.ok(find(busy, "Spinner"));
  assert.equal(icon.props.label, "Delete x");
  assert.equal(icon.props.size, "icon-sm");
  assert.deepEqual(find(icon, "Icon").props, { name: "trash" });
  assert.match(text(last()), /Projectwings/);
  assert.match(text(last()), /Checked 3 h ago/);
});

test("a chip is a button the app draws as a chip, pressed or not", () => {
  mount(h(Page, null, h(Chip, { pressed: true, onClick: () => {} }, "Mon"), h(Chip, { onClick: () => {} }, "Tue")));
  const [on, off] = all(last(), "Button");
  assert.deepEqual([on.props.chip, on.props.pressed, off.props.pressed], [true, true, false]);
});

test("a combobox lists the options that match as chips, a menu opens its items, and useAction keeps busy and error", async () => {
  const picked = [];
  let action;
  function Probe() {
    action = useAction();
    return h(Stack, null,
      h(Combobox, { label: "Base", freeText: true, value: "dev", onInput: () => {}, onPick: (o) => picked.push(o.value), options: [{ value: "develop", label: "develop" }, { value: "main", label: "main" }] }),
      h(Menu, { label: "Start", items: [{ id: "a", label: "In A", onSelect: () => picked.push("a") }] }),
      h(Menu, { label: "⋯", size: "icon-xs", variant: "ghost", "aria-label": "More for notes.txt", items: [] }));
  }
  mount(h(Probe));
  const chips = all(last(), "Button").filter((button) => text(button) === "develop");
  assert.equal(chips.length, 1);
  assert.equal(all(last(), "Button").some((button) => text(button) === "main"), false);
  globalThis.__wingsEvent(chips[0].props.onClick, "");
  assert.deepEqual(picked, ["develop"]);
  assert.equal(all(last(), "Button").some((button) => text(button) === "In A"), false);
  // The trigger is the desktop's, at its size and with its name, and no caret after its label.
  const more = all(last(), "Button").find((button) => button.props.label === "More for notes.txt");
  assert.equal(text(more), "⋯");
  assert.equal(more.props.size, "icon-xs");
  assert.equal(all(last(), "Button").some((button) => text(button) === "Start"), true);
  globalThis.__wingsEvent(all(last(), "Button").find((button) => text(button) === "Start").props.onClick, "");
  const item = all(last(), "Button").find((button) => text(button) === "In A");
  globalThis.__wingsEvent(item.props.onClick, "");
  assert.deepEqual(picked, ["develop", "a"]);
  assert.equal(all(last(), "Button").some((button) => text(button) === "In A"), false);
  assert.equal(await action.run(() => { throw new Error("nope"); }), false);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(action.error, "nope");
});

test("a dot takes a CSS colour from data, a sunburst sends compact wedges and hears the pointer and clicks, and a split names its panes", () => {
  const heard = [];
  function Probe() {
    return h(Split, { defaultSize: 320, start: h(Scroll, null, h(Bar, { edge: "bottom", gap: "xs" }, "Head"), h(Title, { truncate: true }, "Code"), h(Dot, { color: "rgb(17, 34, 51)" }), h(Dot, { color: "#0a0b0c" }), h(Dot, { color: "red" })), end: h(Sunburst, {
      rings: 2, selected: 1, title: "Code", detail: "2 GB", up: true,
      wedges: [{ from: 0, to: 3, ring: 0, color: "rgb(255, 0, 0)" }, { from: 3, to: 6, ring: 0, color: "rgba(0, 0, 255, 0.5)", alpha: 0.5 }],
      onPoint: (index) => heard.push(["point", index]), onSelect: (index, event) => heard.push(["select", index, event.meta]) }) });
  }
  mount(h(Probe));
  assert.deepEqual(find(last(), "Split").props, { defaultSize: 320 });
  assert.deepEqual(all(last(), "Dot").map((dot) => dot.props.color), [0x112233, 0x0a0b0c, undefined]);
  assert.equal(find(last(), "Title"), undefined);
  assert.deepEqual(find(find(last(), "Scroll"), "Text").props, { weight: "strong", truncate: true });
  assert.deepEqual(find(last(), "Bar").props, { edge: "bottom", gap: "xs" });
  const map = find(last(), "Sunburst");
  assert.deepEqual(map.props.wedges, [[0, 3, 0, 0xff0000, 1], [3, 6, 0, 0x0000ff, 0.25]]);
  assert.deepEqual([map.props.rings, map.props.selected, map.props.title, map.props.up], [2, 1, "Code", true]);
  globalThis.__wingsEvent(map.props.onPoint, '{"index":-2}');
  globalThis.__wingsEvent(map.props.onSelect, '{"index":0,"meta":true}');
  assert.deepEqual(heard, [["point", -2], ["select", 0, true]]);
});

test("an svg sends its shapes as data, a fragment's and a group's included, and only the props it documents", () => {
  mount(h(Svg, { viewBox: "0 0 16 16", size: 18, tone: "warn", label: "Epic", fill: "none", stroke: "currentColor", strokeWidth: "1.4", className: "x", onClick() {} },
    h(Fragment, null, h("rect", { x: "2.5", y: 2.5, width: 11, height: 11, rx: "2.5" }), h("path", { d: "m5.5 8.2 1.8 1.8" })),
    [h("g", { key: "g", opacity: 0.3 }, h("circle", { cx: 8, cy: 8, r: 2 }))],
    "text the app has no shape for",
    null));
  const svg = find(last(), "Svg");
  assert.deepEqual(svg.children, []);
  assert.deepEqual(svg.props, {
    viewBox: "0 0 16 16", size: 18, tone: "warn", label: "Epic", fill: "none", stroke: "currentColor", strokeWidth: "1.4",
    shapes: [["rect", { x: "2.5", y: 2.5, width: 11, height: 11, rx: "2.5" }], ["path", { d: "m5.5 8.2 1.8 1.8" }], ["g", { opacity: 0.3 }, [["circle", { cx: 8, cy: 8, r: 2 }]]]],
  });
});

test("tab state is null until the app's saved state is read through check, and a set saves it", async () => {
  const saved = [];
  globalThis.wings.tabState = async () => ({ folded: ["a", 1] });
  globalThis.wings.setTabState = async (state) => void saved.push(state);
  let tab;
  function Probe() {
    tab = useTabState({ folded: [] }, (raw) => (Array.isArray(raw?.folded) ? { folded: raw.folded.filter((k) => typeof k === "string") } : null));
    return h(Text, null, JSON.stringify(tab[0]));
  }
  mount(h(Probe));
  assert.equal(tab[0], null);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(tab[0], { folded: ["a"] });
  tab[1]({ folded: [] });
  assert.deepEqual(saved, [{ folded: [] }]);
});

test("a timeline card and note show the author, the time and what was said", () => {
  mount(h(Timeline, null,
    h(TimelineCard, { author: "Ada Lovelace", at: new Date(Date.now() - 5 * 60_000).toISOString(), tag: "Author" }, h(Text, null, "Looks good")),
    h(TimelineNote, { author: "Grace Hopper" }, "changed the state")));
  const [card, note] = [find(last(), "Card"), all(last(), "Row").at(-1)];
  assert.equal(text(card), "Ada LovelaceAuthor5 min agoLooks good");
  assert.equal(find(card, "Avatar").props.name, "Ada Lovelace");
  assert.equal(text(note), "Grace Hopperchanged the state");
});

test("figures sit side by side with the label over the value, and a heading puts its actions on its right", async () => {
  const { Stats } = await import("./native.js");
  mount(h(Stack, null,
    h(Stats, { items: [{ label: "Status", value: "Working" }, { label: "Context", value: "116k", tone: "warn" }] }),
    h(Heading, { actions: h(Button, null, "Filter") }, "Recent activity"),
    h(Heading, null, "Plain"),
    h(Heading, { level: 3 }, "Section")));
  const [figures, heading] = all(last(), "Row");
  assert.deepEqual(figures.props, { gap: "l", wrap: true });
  assert.deepEqual(figures.children.map((figure) => figure.children.map((part) => [part.props.tone, part.props.size, text(part)])), [
    [["muted", "tiny", "Status"], [undefined, undefined, "Working"]],
    [["muted", "tiny", "Context"], ["warn", undefined, "116k"]],
  ]);
  assert.deepEqual(heading.props, { justify: "between", wrap: true });
  assert.deepEqual(heading.children.map((child) => child.type), ["Heading", "Button"]);
  assert.equal(all(last(), "Heading").length, 3);
  // Level 3 names a section of a page, and the app draws it as one.
  assert.equal(all(last(), "Heading")[2].props.level, 3);
  assert.equal(all(last(), "Row").length, 2, "a heading with no actions has no row around it");
});

test("pull request pages: avatars, skeletons, summary chips, tabs and a list beside its detail", () => {
  const picks = [];
  mount(h(Stack, null,
    h(Avatar, { name: "Ada Lovelace", size: "small" }),
    h(Skeletons, { label: "Loading" }, h(Skeleton, { width: "40%", height: 14 }), h(Skeleton, { height: 90 })),
    h(Summary, null, h(SummaryChip, { tone: "success" }, "2 approved"), h(SummaryChip, { tone: "danger", onClick: () => picks.push("checks") }, "1 failed ›")),
    h(Tabs, { label: "Pull request", items: [{ value: "a", label: "Overview" }, { value: "b", label: "Checks" }], value: "a", onChange: (value) => picks.push(value) }, h(Text, null, "Panel")),
    h(ListDetail, { list: h(Text, null, "Steps"), detail: h(Text, null, "Log"), listWidth: 300 })));
  assert.deepEqual(find(last(), "Avatar").props, { name: "Ada Lovelace", size: "small" });
  assert.deepEqual(all(last(), "Skeleton").map((skeleton) => skeleton.props), [{ width: "40%", height: 14 }, { height: 90 }]);
  assert.equal(find(find(last(), "Row"), "Badge").props.variant, "outline");
  globalThis.__wingsEvent(find(last(), "Button").props.onClick, "");
  globalThis.__wingsEvent(find(last(), "Chips").props.onChange, '{"value":"b"}');
  assert.deepEqual(picks, ["checks", "b"]);
  assert.equal(text(last()).includes("Panel"), true);
  const [, pane] = all(last(), "Stack").filter((stack) => stack.props.width === 300 || stack.props.grow);
  assert.equal(text(pane), "Log");
  assert.equal(all(last(), "Stack").some((stack) => stack.props.width === 300), true);
});

test("Markdown draws headings, lists, code, tables and folds, with bold, code and links as spans of their line", () => {
  const opened = [];
  globalThis.wings.openUrl = async (url) => void opened.push(url);
  mount(h(Markdown, { source: "# Title\n\nSome **bold** and `code` with [a link](https://example.com/x).\n\n- one\n- [x] done\n\n```ts\nconst a = 1;\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n<details><summary>More</summary>\n\nhidden\n\n</details>" }));
  const body = last().children[0];
  assert.equal(body.type, "Stack");
  const paragraph = all(body, "Text").find((node) => text(node).startsWith("Some"));
  assert.deepEqual(paragraph.children.filter((child) => child.type === "Text").map((span) => [span.props.weight, span.props.mono, text(span)]), [["strong", undefined, "bold"], [undefined, true, "code"], [undefined, undefined, "a link"]]);
  const link = paragraph.children.find((child) => child.type === "Text" && text(child) === "a link");
  globalThis.__wingsEvent(link.props.onClick, "");
  assert.deepEqual(opened, ["https://example.com/x"]);
  assert.equal(text(body).includes("const a = 1;"), true);
  assert.equal(find(body, "Disclosure") !== undefined, true);
  assert.deepEqual(all(body, "Text").filter((node) => ["A", "B", "1", "2"].includes(text(node))).map(text), ["A", "B", "1", "2"]);
});

test("Markdown list markers stay at the first line of wrapped bullet, ordered, task and nested items", () => {
  mount(h(Markdown, { source: "- A long bullet item that wraps at narrow page widths\n1. A long ordered item that wraps at narrow page widths\n- [ ] A long task item that wraps at narrow page widths\n  - A long nested item that wraps at narrow page widths" }));
  const rows = all(last(), "Row");
  assert.deepEqual(rows.map((row) => row.props.align), ["start", "start", "start", "start"]);
  assert.deepEqual(rows.map((row) => text(row.children.find((child) => child.type === "Text" && !child.props.grow))), ["•", "1.", "☐", "•"]);
  assert.equal(rows.at(-1).children.some((child) => child.type === "Stack" && child.props.width === 16), true);
  assert.ok(rows.every((row) => row.children.some((child) => child.type === "Text" && child.props.grow)));
});

test("a project gate waits for the project, and a tab reads what openView opened it with", async () => {
  globalThis.wings.project = async () => ({ id: "/w", name: "web", path: "/w" });
  globalThis.wings.viewArgs = () => ({ url: "https://example.com/pull/1" });
  function Args() {
    const args = useViewArgs((raw) => (typeof raw?.url === "string" ? { url: raw.url } : null));
    return h(Text, null, args?.url ?? "none");
  }
  mount(h(ProjectGate, { loading: "Loading project" }, (project) => h(Stack, null, h(Text, null, project.name), h(Args))));
  assert.equal(text(last()), "Loading project");
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(text(last()), "webhttps://example.com/pull/1");
});

test("a code block numbers its lines and marks added, removed and highlighted ones", () => {
  mount(h(CodeBlock, { lines: [{ text: "a", n: 1 }, { text: "b", n: 2, kind: "added" }, { text: "c", n: 3, kind: "removed", highlighted: true }] }));
  assert.deepEqual(all(last(), "Text").filter((node) => node.props.mono && !node.props.numeric).map((node) => [text(node), node.props.tone, node.props.weight]), [["a", undefined, undefined], ["b", "success", undefined], ["c", "danger", "strong"]]);
});

test("mount puts fill on the tree's root, so the app lays the page out edge to edge, and leaves it off otherwise", () => {
  mount(h(Page, null, "Edge"), { fill: true });
  assert.deepEqual(last().props, { fill: true });
  mount(h(Page, null, "Padded"));
  assert.deepEqual(last().props, {});
});
