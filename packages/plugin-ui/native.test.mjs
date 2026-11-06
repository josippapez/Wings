// node --test: what the native components render, as the JSON tree the app draws.
import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement as h, useEffect, useState } from "react";

const trees = [];
globalThis.wings = { render: (json) => trees.push(JSON.parse(json)) };
const { Badge, Button, Card, Checkbox, Chips, Confirm, Disclosure, Dot, Empty, Gauge, Graph, Heading, List, ListButton, ListItem, Loading, Notice, Page, Row, SearchField, Select, Spinner, Stack, Text, Toolbar, mount } = await import("./native.js");

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
