// node --test: how the ui components behave in a page, in jsdom. React DOM checks for a DOM once, when it loads, so
// the page is set up before anything imports it.
import assert from "node:assert/strict";
import { after, test } from "node:test";
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><body></body>", { pretendToBeVisual: true });
// The page's animation frames keep the process alive until it's closed.
after(() => dom.window.close());
// Base UI (and floating-ui under it) reads the DOM's classes as globals, like a browser page has them.
for (const key of Object.getOwnPropertyNames(dom.window)) {
  if (!(key in globalThis) || key === "navigator") Object.defineProperty(globalThis, key, { configurable: true, get: () => dom.window[key] });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
// jsdom has no layout, so nothing to scroll or measure; browsers have both.
dom.window.Element.prototype.scrollIntoView = () => {};
dom.window.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };
const { act, createElement: h, useCallback, useEffect, useState } = await import("react");
const { createRoot } = await import("react-dom/client");
const { Board, BoardCell, BoardColumn, BoardLane, BoardLanes, Button, Checkbox, Chips, Combobox, Disclosure, Field, FieldGroup, ListDetail, Markdown, Menu, ProjectGate, Select, Split, Tabs, TextInput, TimelineCard, copyText, showBlock, useAction, useBlockState, usePoll, useTabState, useViewArgs } = await import("./ui.js");

/** Renders `element` into the page, and gives back its host and ways to render again and to unmount. */
async function mount(element) {
  const host = document.body.appendChild(document.createElement("div"));
  const root = createRoot(host);
  await act(() => root.render(element));
  return { host, render: (next) => act(() => root.render(next)), unmount: () => act(() => root.unmount()) };
}

/** Lets what Base UI schedules after an event (animation frames, timeouts) run inside act. */
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 10)); });
/**
 * A mouse press on `el` the way a browser sends it: pointer, mouse, then click, each its own event-loop task, so
 * React renders between them as it does in a page.
 */
const click = async (el) => {
  for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]) {
    const Event = type.startsWith("pointer") ? dom.window.PointerEvent : dom.window.MouseEvent;
    // Browsers mark a mouse's pointer events primary; Base UI reads it to tell a click from a drag-select.
    await act(async () => {
      el.dispatchEvent(new Event(type, { bubbles: true, cancelable: true, button: 0, pointerType: "mouse", isPrimary: true }));
      // A browser runs pending timers between the events of a click; Base UI sets state from some.
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
};
/** Waits, letting the page run, until `done()` is true; fails after two seconds. */
async function until(done, what) {
  for (const start = Date.now(); !done(); ) {
    if (Date.now() - start > 2000) throw new Error(`timed out waiting for ${what}`);
    await settle();
  }
}
/** The text of the option the field's `aria-activedescendant` points at. */
const highlightedText = (input) => document.getElementById(input.getAttribute("aria-activedescendant"))?.textContent ?? null;

const options = [{ value: "1", label: "Wings" }, { value: "2", label: "Cyclops" }];

test("Combobox opens its list when it gets focus, unless openOnFocus is false; a click opens it then", async () => {
  for (const [openOnFocus, expected] of [[undefined, "true"], [false, "false"]]) {
    const { host, unmount } = await mount(h(Combobox, { id: "p", label: "Project", value: "", options, onPick() {}, openOnFocus }));
    const input = host.querySelector("input");
    await act(() => input.focus());
    assert.equal(input.getAttribute("aria-expanded"), expected, `openOnFocus ${openOnFocus}`);
    await click(input);
    assert.equal(input.getAttribute("aria-expanded"), "true");
    await unmount();
  }
});

test("a busy Button keeps focus, says it's busy, shows a spinner and ignores clicks", async () => {
  let clicks = 0;
  const button = (busy) => h(Button, { busy, onClick: () => clicks++ }, "Save");
  const { host, render, unmount } = await mount(button(false));
  const el = host.querySelector("button");
  el.focus();
  await render(button(true));
  assert.equal(document.activeElement, el);
  assert.equal(el.getAttribute("aria-busy"), "true");
  assert.equal(el.getAttribute("aria-disabled"), "true");
  assert.equal(el.disabled, false);
  assert.ok(el.querySelector(".w-spinner"));
  await act(() => el.click());
  assert.equal(clicks, 0);
  await render(button(false));
  await act(() => el.click());
  assert.equal(clicks, 1);
  assert.equal(el.querySelector(".w-spinner"), null);
  await unmount();
});

/** Presses `key` on `el`, and says whether the page's default for it was left alone. */
const press = (el, key) => act(() => el.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key, bubbles: true, cancelable: true })));
const pressed = async (el, key) => {
  let event;
  el.addEventListener("keydown", (e) => (event = e), { once: true });
  await press(el, key);
  return event;
};

test("Combobox calls the onKeyDown you pass first, and skips its own keys when you preventDefault", async () => {
  const picked = [];
  const seen = [];
  const combobox = (onKeyDown) => h(Combobox, { label: "Project", value: "", options, onPick: (o) => picked.push(o.value), onKeyDown });
  const { host, render, unmount } = await mount(combobox((e) => seen.push(e.key)));
  const input = host.querySelector("input");
  await act(() => input.focus());
  await press(input, "Enter");
  assert.deepEqual(seen, ["Enter"]);
  assert.deepEqual(picked, ["1"], "without preventDefault the Combobox still picks");
  await render(combobox((e) => e.key === "ArrowDown" && e.preventDefault()));
  await act(() => input.blur());
  await act(() => input.focus());
  await press(input, "ArrowDown");
  assert.equal(highlightedText(input), "Wings", "the highlighted option didn't move");
  await unmount();
});

test("Combobox onEnter gets the typed text when nothing is highlighted, and Enter doesn't submit", async () => {
  const entered = [];
  const picked = [];
  const { host, unmount } = await mount(h(Combobox, { label: "People", value: "n", freeText: true, options, onPick: (o) => picked.push(o.value), onEnter: (text) => entered.push(text) }));
  const input = host.querySelector("input");
  await act(() => input.focus());
  assert.equal(input.getAttribute("aria-activedescendant"), null, "free text starts with nothing highlighted");
  assert.equal((await pressed(input, "Enter")).defaultPrevented, true);
  assert.deepEqual(entered, ["n"]);
  await act(() => input.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
  await press(input, "Enter");
  assert.deepEqual(entered, ["n"], "a highlighted option is picked instead");
  assert.equal(picked.length, 1);
  await unmount();
});

test("Combobox without onEnter leaves Enter alone when nothing is highlighted", async () => {
  const { host, unmount } = await mount(h(Combobox, { label: "People", value: "ann", freeText: true, options, onPick() {} }));
  const input = host.querySelector("input");
  await act(() => input.focus());
  assert.equal((await pressed(input, "Enter")).defaultPrevented, false);
  await unmount();
});

test("a Combobox picker selects the picked label, on focus and after a pick, so typing replaces it", async () => {
  function Picker() {
    const [value, setValue] = useState("Cyclops");
    return h(Combobox, { label: "Project", value, options, openOnFocus: false, onPick: (o) => setValue(o.label) });
  }
  const { host, unmount } = await mount(h(Picker));
  const input = host.querySelector("input");
  const selected = () => input.value.slice(input.selectionStart, input.selectionEnd);
  await act(() => input.focus());
  assert.equal(selected(), "Cyclops", "on focus");
  await press(input, "ArrowDown");
  await press(input, "ArrowDown");
  await press(input, "Enter");
  assert.equal(input.value, "Cyclops");
  assert.equal(selected(), "Cyclops", "after a pick");
  await press(input, "ArrowDown");
  await press(input, "Escape");
  assert.equal(selected(), "Cyclops", "after Escape");
  await unmount();
});

test("ArrowDown on a closed Combobox opens it and highlights the first option", async () => {
  const { host, unmount } = await mount(h(Combobox, { id: "p", label: "Project", value: "", options, openOnFocus: false, onPick() {} }));
  const input = host.querySelector("input");
  await act(() => input.focus());
  assert.equal(input.getAttribute("aria-expanded"), "false");
  await press(input, "ArrowDown");
  assert.equal(input.getAttribute("aria-expanded"), "true");
  assert.equal(highlightedText(input), "Wings");
  await unmount();
});

/** A free-text Combobox whose text is held here, like a search field in a page. */
function Search({ open }) {
  const [value, setValue] = useState("");
  return h("div", null, h(Combobox, { id: "s", label: "People", value, onInput: setValue, freeText: true, open, options, filter: () => true, onPick() {} }), h("button", { id: "other" }, "other"));
}
const type = (input, text) => act(() => {
  Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, "value").set.call(input, text);
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
});
const shownOptions = () => document.querySelectorAll("[role=option]").length;

test('Combobox open="whileText" keeps the list through Escape, Tab away and a press outside, and closes when emptied', async () => {
  const { host, unmount } = await mount(h(Search, { open: "whileText" }));
  const input = host.querySelector("input");
  await act(() => input.focus());
  await type(input, "da");
  assert.equal(input.getAttribute("aria-expanded"), "true");
  assert.equal(shownOptions(), 2);
  await press(input, "ArrowDown");
  assert.equal(highlightedText(input), "Wings");
  await press(input, "Escape");
  assert.equal(input.getAttribute("aria-activedescendant"), null, "Escape clears the highlight");
  assert.equal(input.getAttribute("aria-expanded"), "true", "after Escape");
  assert.equal(shownOptions(), 2);
  await press(input, "Tab");
  await act(() => input.dispatchEvent(new dom.window.FocusEvent("focusout", { bubbles: true, relatedTarget: host.querySelector("#other") })));
  await act(() => input.blur());
  assert.equal(input.getAttribute("aria-expanded"), "true", "after Tab and blur");
  assert.equal(shownOptions(), 2);
  await act(() => document.body.dispatchEvent(new dom.window.MouseEvent("mousedown", { bubbles: true })));
  assert.equal(input.getAttribute("aria-expanded"), "true", "after a press outside");
  assert.equal(shownOptions(), 2);
  await type(input, "");
  assert.equal(input.getAttribute("aria-expanded"), "false", "emptied");
  assert.equal(shownOptions(), 0);
  await unmount();
});

test('Combobox open="whileText" is closed when nothing matches, and Escape with nothing highlighted is left to the page', async () => {
  const none = (host) => host.querySelector("input").getAttribute("aria-expanded");
  function None() {
    const [value, setValue] = useState("");
    return h(Combobox, { label: "People", value, onInput: setValue, freeText: true, open: "whileText", options, onPick() {} });
  }
  const { host, unmount } = await mount(h(None));
  const input = host.querySelector("input");
  await type(input, "zzz");
  assert.equal(none(host), "false");
  await type(input, "wi");
  assert.equal(none(host), "true");
  assert.equal((await pressed(input, "Escape")).defaultPrevented, false);
  await unmount();
});

test('Combobox open="auto" still closes on Escape, Tab, blur and a press outside', async () => {
  for (const dismiss of [
    (input) => press(input, "Escape"),
    // Tab moves focus on, which is what closes the list; jsdom doesn't move it for a key.
    async (input, host) => {
      await press(input, "Tab");
      await act(() => host.querySelector("#other").focus());
    },
    () => click(document.body),
  ]) {
    const { host, unmount } = await mount(h(Search, { open: undefined }));
    const input = host.querySelector("input");
    await act(() => input.focus());
    await type(input, "da");
    assert.equal(input.getAttribute("aria-expanded"), "true");
    await dismiss(input, host);
    await settle();
    assert.equal(input.getAttribute("aria-expanded"), "false");
    assert.equal(shownOptions(), 0);
    await unmount();
  }
});

test("Field ties its label to the input, with the id you pass, and describes it by its hint", async () => {
  const { host, unmount } = await mount(h(Field, { id: "f-project", label: "Project", hint: "Pick one" }, h(TextInput)));
  const input = host.querySelector("input");
  const label = host.querySelector("label");
  assert.equal(input.id, "f-project");
  assert.equal(label.htmlFor, "f-project");
  assert.equal(input.getAttribute("aria-describedby"), "f-project-hint");
  assert.equal(host.querySelector("#f-project-hint").textContent, "Pick one");
  await unmount();
});

test("Field without an id still ties its label to its input, and an error marks the input invalid", async () => {
  const { host, unmount } = await mount(h(Field, { label: "Name", error: "Required" }, h(TextInput)));
  const input = host.querySelector("input");
  assert.ok(input.id);
  assert.equal(host.querySelector("label").htmlFor, input.id);
  assert.equal(input.getAttribute("aria-invalid"), "true");
  assert.match(input.getAttribute("aria-describedby") ?? "", /-hint/);
  await unmount();
});

test("Field inline puts the label and the input in one row", async () => {
  const { host, unmount } = await mount(h(Field, { id: "x", label: "A", inline: true }, h(TextInput)));
  assert.ok(host.querySelector(".w-field[data-layout=inline] > .w-field-row > label + input#x"));
  await unmount();
});

test("FieldGroup is a fieldset described by its hint, and an error marks each checkbox invalid", async () => {
  const group = (props) => h(FieldGroup, { label: "States", ...props }, h(Checkbox, null, "Active"));
  const ok = await mount(group({ hint: "Shown on the board" }));
  const fieldset = ok.host.querySelector("fieldset");
  assert.equal(fieldset.getAttribute("aria-describedby"), ok.host.querySelector(".w-field-hint").id);
  // Base UI's legend is a div that names the fieldset through aria-labelledby.
  assert.equal(document.getElementById(fieldset.getAttribute("aria-labelledby")).textContent, "States");
  assert.equal(ok.host.querySelector("[role=checkbox]").hasAttribute("aria-invalid"), false);
  await ok.unmount();
  const bad = await mount(group({ error: "Pick at least one" }));
  assert.equal(bad.host.querySelector("[role=checkbox]").getAttribute("aria-invalid"), "true");
  assert.equal(bad.host.querySelector(".w-field-hint").dataset.tone, "danger");
  await bad.unmount();
});

test("Checkbox tells onChange whether it's checked, from a click on the box or on its label, and its id is the focusable box", async () => {
  const seen = [];
  const { host, unmount } = await mount(h(Checkbox, { id: "all", onChange: (checked) => seen.push(checked) }, "All branches"));
  const box = document.getElementById("all");
  assert.equal(box.getAttribute("role"), "checkbox");
  assert.equal(box.tagName, "BUTTON");
  await click(box);
  await click(host.querySelector("label"));
  assert.deepEqual(seen, [true, false]);
  await act(() => box.focus());
  assert.equal(document.activeElement, box);
  await unmount();
});

test("Chips call onChange with the chip clicked, the one that's on too, and mark that one pressed", async () => {
  const seen = [];
  const options = [{ value: "a", label: "All" }, { value: "b", label: "Open", id: "chip-b" }];
  function Filter() {
    const [value, setValue] = useState("a");
    return h(Chips, { label: "Show", options, value, onChange: (v) => { seen.push(v); setValue(v); } });
  }
  const { host, unmount } = await mount(h(Filter));
  const [a, b] = host.querySelectorAll("button");
  assert.equal(b.id, "chip-b");
  assert.equal(a.getAttribute("aria-pressed"), "true");
  await click(b);
  assert.equal(b.getAttribute("aria-pressed"), "true");
  assert.equal(a.getAttribute("aria-pressed"), "false");
  await click(b);
  assert.deepEqual(seen, ["b", "b"]);
  assert.equal(b.getAttribute("aria-pressed"), "true", "pressing the chip that's on keeps it on");
  await unmount();
});

test("Tabs show the panel of the tab picked, and arrow keys, Home and End move to a tab and show it", async () => {
  function Views() {
    const [value, setValue] = useState("one");
    return h(Tabs, { label: "Views", items: [{ value: "one", label: "One" }, { value: "two", label: "Two" }, { value: "three", label: "Three" }], value, onChange: setValue }, h("p", null, `Panel ${value}`));
  }
  const { host, unmount } = await mount(h(Views));
  const tabs = () => [...host.querySelectorAll("[role=tab]")];
  assert.equal(host.querySelector("[role=tablist]").getAttribute("aria-label"), "Views");
  assert.equal(host.querySelector("[role=tabpanel]").textContent, "Panel one");
  assert.equal(host.querySelector("[role=tabpanel]").getAttribute("aria-labelledby"), tabs()[0].id);
  await act(() => tabs()[0].focus());
  for (const [key, expected] of [["ArrowRight", "two"], ["ArrowRight", "three"], ["ArrowRight", "one"], ["ArrowLeft", "three"], ["Home", "one"], ["End", "three"]]) {
    await press(document.activeElement, key);
    await settle();
    assert.equal(host.querySelector("[role=tabpanel]").textContent, `Panel ${expected}`, key);
    assert.equal(document.activeElement.getAttribute("aria-selected"), "true", `${key}: focus is on the selected tab`);
  }
  await click(tabs()[1]);
  assert.equal(host.querySelector("[role=tabpanel]").textContent, "Panel two");
  await unmount();
});

const menuItems = (picked) => ["Rename", "Duplicate", "Delete"].map((label) => ({ id: label, label, onSelect: () => picked.push(label), variant: label === "Delete" ? "destructive" : undefined }));
const items = () => [...document.querySelectorAll("[role=menuitem]")];

test("Menu opens under its button; arrows, Home and End move; Enter picks; Escape closes and puts focus back", async () => {
  const picked = [];
  const { host, unmount } = await mount(h(Menu, { label: "More", items: menuItems(picked) }));
  const button = host.querySelector("button");
  assert.equal(button.getAttribute("aria-haspopup"), "menu");
  await act(() => button.focus());
  await press(button, "ArrowDown");
  await settle();
  assert.equal(button.getAttribute("aria-expanded"), "true");
  assert.deepEqual(items().map((i) => i.textContent), ["Rename", "Duplicate", "Delete"]);
  assert.equal(items()[2].dataset.variant, "destructive");
  assert.equal(document.activeElement, items()[0]);
  await press(document.activeElement, "ArrowDown");
  assert.equal(document.activeElement, items()[1]);
  await press(document.activeElement, "End");
  assert.equal(document.activeElement, items()[2]);
  await press(document.activeElement, "Home");
  assert.equal(document.activeElement, items()[0]);
  // Wrapping from the first item to the last needs layout, which jsdom lacks; it's checked in a browser.
  await press(document.activeElement, "Escape");
  await settle();
  assert.equal(button.getAttribute("aria-expanded"), "false");
  assert.equal(document.activeElement, button);
  await press(button, "ArrowDown");
  await settle();
  await press(document.activeElement, "ArrowDown");
  await press(document.activeElement, "Enter");
  await settle();
  assert.deepEqual(picked, ["Duplicate"]);
  assert.equal(button.getAttribute("aria-expanded"), "false", "picking closes it");
  await unmount();
});

test("Menu items can be picked with a click", async () => {
  const picked = [];
  const { host, unmount } = await mount(h(Menu, { label: "More", items: menuItems(picked) }));
  await click(host.querySelector("button"));
  await until(() => items().length > 0, "the menu to open");
  await click(items()[0]);
  await settle();
  assert.deepEqual(picked, ["Rename"]);
  await unmount();
});

test("Select shows the option picked, opens a list in the page, and tells onChange the value", async () => {
  const seen = [];
  const options = [{ value: "a", label: "Alpha" }, { value: "b", label: "Bravo" }, { value: "c", label: "Charlie", disabled: true }];
  function Pick() {
    const [value, setValue] = useState("a");
    return h(Select, { label: "Pick", id: "pick", options, value, onChange: (v) => { seen.push(v); setValue(v); } });
  }
  const { host, unmount } = await mount(h(Pick));
  const trigger = document.getElementById("pick");
  assert.equal(trigger.textContent, "Alpha");
  assert.equal(trigger.getAttribute("aria-label"), "Pick");
  await click(trigger);
  await until(() => document.querySelector("[role=option]"), "the list to open");
  const choices = [...document.querySelectorAll("[role=option]")];
  assert.deepEqual(choices.map((o) => o.textContent), ["Alpha", "Bravo", "Charlie"]);
  assert.ok(!host.contains(choices[0]), "the list is in the page, not in the select's place");
  assert.equal(choices[2].getAttribute("aria-disabled"), "true");
  await click(choices[1]);
  await settle();
  assert.deepEqual(seen, ["b"]);
  assert.equal(trigger.textContent, "Bravo");
  // The list leaves the page when its closing animation ends, which jsdom never runs; the trigger says it's closed.
  assert.equal(trigger.getAttribute("aria-expanded"), "false", "picking closes it");
  await unmount();
});

test("a Select keeps a pick when the page drops an option before it in the same render, like a Choose… row", async () => {
  const seen = [];
  function Pick() {
    const [value, setValue] = useState("");
    const options = [...(value === "" ? [{ value: "", label: "Choose…", disabled: true }] : []), { value: "a", label: "Alpha" }, { value: "b", label: "Bravo" }];
    return h(Select, { label: "Pick", id: "pick", options, value, onChange: (v) => { seen.push(v); setValue(v); } });
  }
  const { unmount } = await mount(h(Pick));
  const trigger = document.getElementById("pick");
  await click(trigger);
  await until(() => document.querySelector("[role=option]"), "the list to open");
  await click([...document.querySelectorAll("[role=option]")].find((o) => o.textContent === "Bravo"));
  await settle();
  assert.deepEqual(seen, ["b"]);
  assert.equal(trigger.textContent, "Bravo");
  await unmount();
});

test("a Select without a value starts on its first option", async () => {
  const { host, unmount } = await mount(h(Select, { label: "Pick", options: [{ value: "a", label: "Alpha" }, { value: "b", label: "Bravo" }] }));
  assert.equal(host.querySelector("[role=combobox]").textContent, "Alpha");
  await unmount();
});

test("Disclosure opens and closes by its summary, and can start open", async () => {
  const { host, unmount } = await mount(h("div", null, h(Disclosure, { summary: "Closed" }, "Hidden"), h(Disclosure, { summary: "Open", defaultOpen: true }, "Shown")));
  const [closed, open] = host.querySelectorAll("button");
  assert.equal(closed.getAttribute("aria-expanded"), "false");
  assert.equal(open.getAttribute("aria-expanded"), "true");
  assert.ok(!host.textContent.includes("Hidden"));
  assert.ok(host.textContent.includes("Shown"));
  await click(closed);
  await settle();
  assert.equal(closed.getAttribute("aria-expanded"), "true");
  assert.ok(host.textContent.includes("Hidden"));
  await unmount();
});

test("Combobox picks an option with a click, filters by label and detail, and says when nothing matches", async () => {
  const picked = [];
  const projects = [{ value: "1", label: "Wings", detail: "Prototyp" }, { value: "2", label: "Cyclops" }];
  const { host, unmount } = await mount(h(Combobox, { id: "p", label: "Project", value: "", options: projects, onPick: (o) => picked.push(o.value), empty: "Nothing" }));
  const input = host.querySelector("input");
  await act(() => input.focus());
  await settle();
  assert.deepEqual([...document.querySelectorAll("[role=option]")].map((o) => o.textContent), ["WingsPrototyp", "Cyclops"]);
  await type(input, "proto");
  await settle();
  assert.deepEqual([...document.querySelectorAll("[role=option]")].map((o) => o.textContent), ["WingsPrototyp"], "the detail matches too");
  await type(input, "zzz");
  await settle();
  assert.equal(document.querySelectorAll("[role=option]").length, 0);
  assert.ok(document.body.textContent.includes("Nothing"));
  await type(input, "cyc");
  await settle();
  await click(document.querySelector("[role=option]"));
  await settle();
  assert.deepEqual(picked, ["2"]);
  assert.equal(input.getAttribute("aria-expanded"), "false");
  await unmount();
});

test('Combobox open="whileText" keeps the list through a press outside, as a real click', async () => {
  const { host, unmount } = await mount(h(Search, { open: "whileText" }));
  const input = host.querySelector("input");
  await type(input, "da");
  await click(host.querySelector("#other"));
  await settle();
  assert.equal(shownOptions(), 2);
  await click(document.body);
  await settle();
  assert.equal(shownOptions(), 2);
  await unmount();
});

const key = (el, k, init = {}) => act(() => { el.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init })); });
const pointer = (el, type, x, init = {}) => act(() => { el.dispatchEvent(new dom.window.PointerEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: x, pointerId: 1, ...init })); });

test("Split is a separator that the arrow keys, Shift, Home and End resize, and tells onResize once per change", async () => {
  const sizes = [];
  const { host, unmount } = await mount(h(Split, { start: "A", end: "B", defaultSize: 200, min: 100, max: 300, onResize: (n) => sizes.push(n) }));
  const handle = host.querySelector("[role=separator]");
  assert.equal(handle.getAttribute("aria-orientation"), "vertical");
  assert.equal(handle.getAttribute("aria-valuenow"), "200");
  assert.equal(handle.getAttribute("aria-valuemin"), "100");
  assert.equal(handle.getAttribute("aria-valuemax"), "300");
  assert.equal(handle.getAttribute("tabindex"), "0");
  assert.equal(handle.getAttribute("aria-controls"), host.querySelector(".w-split-pane").id);
  await key(handle, "ArrowRight");
  assert.equal(handle.getAttribute("aria-valuenow"), "216");
  await key(handle, "ArrowLeft", { shiftKey: true });
  assert.equal(handle.getAttribute("aria-valuenow"), "152");
  await key(handle, "End");
  await key(handle, "End");
  assert.equal(handle.getAttribute("aria-valuenow"), "300");
  await key(handle, "Home");
  assert.equal(handle.getAttribute("aria-valuenow"), "100");
  assert.equal(host.querySelector(".w-split").style.getPropertyValue("--w-split-size"), "100px");
  assert.deepEqual(sizes, [216, 152, 300, 100], "a key that changes nothing says nothing");
  await unmount();
});

test("Split vertical answers to the up and down arrows only", async () => {
  const { host, unmount } = await mount(h(Split, { start: "A", end: "B", direction: "vertical", defaultSize: 200 }));
  const handle = host.querySelector("[role=separator]");
  assert.equal(handle.getAttribute("aria-orientation"), "horizontal");
  await key(handle, "ArrowRight");
  assert.equal(handle.getAttribute("aria-valuenow"), "200");
  await key(handle, "ArrowDown");
  assert.equal(handle.getAttribute("aria-valuenow"), "216");
  await unmount();
});

test("Split drag moves the CSS variable and the handle's value, and commits to onResize on release only", async () => {
  const sizes = [];
  let renders = 0;
  const Start = () => { renders++; return "A"; };
  const { host, unmount } = await mount(h(Split, { start: h(Start), end: "B", defaultSize: 200, min: 100, max: 400, onResize: (n) => sizes.push(n) }));
  const handle = host.querySelector("[role=separator]");
  const split = host.querySelector(".w-split");
  const before = renders;
  await pointer(handle, "pointerdown", 500);
  assert.ok(handle.hasAttribute("data-dragging"));
  await pointer(handle, "pointermove", 530);
  await pointer(handle, "pointermove", 560);
  assert.equal(split.style.getPropertyValue("--w-split-size"), "260px");
  assert.equal(handle.getAttribute("aria-valuenow"), "260");
  assert.deepEqual(sizes, [], "nothing is committed mid-drag");
  assert.equal(renders, before, "no render mid-drag");
  await pointer(handle, "pointermove", 5000);
  assert.equal(handle.getAttribute("aria-valuenow"), "400", "held at max");
  await pointer(handle, "pointerup", 5000);
  assert.ok(!handle.hasAttribute("data-dragging"));
  assert.deepEqual(sizes, [400]);
  await unmount();
});

test("Split drag that is cancelled puts the size back", async () => {
  const sizes = [];
  const { host, unmount } = await mount(h(Split, { start: "A", end: "B", defaultSize: 200, onResize: (n) => sizes.push(n) }));
  const handle = host.querySelector("[role=separator]");
  await pointer(handle, "pointerdown", 100);
  await pointer(handle, "pointermove", 160);
  await pointer(handle, "pointercancel", 160);
  assert.equal(handle.getAttribute("aria-valuenow"), "200");
  assert.equal(host.querySelector(".w-split").style.getPropertyValue("--w-split-size"), "200px");
  assert.deepEqual(sizes, []);
  await unmount();
});

test("ListDetail shows the empty placeholder until there is a detail, then the detail and a back button", async () => {
  let backs = 0;
  const { host, render, unmount } = await mount(h(ListDetail, { list: h("p", null, "Rows"), listLabel: "Tasks", detailLabel: "Task", onBack: () => backs++ }));
  const root = host.querySelector(".w-list-detail");
  assert.equal(root.style.getPropertyValue("--w-list-width"), "280px");
  assert.ok(!root.hasAttribute("data-has-detail"));
  assert.equal(host.querySelector("section[aria-label=Tasks]").textContent, "Rows");
  assert.equal(host.querySelector("section[aria-label=Task]").textContent, "Nothing selected");
  assert.equal(host.querySelector(".w-list-detail-back"), null);
  await render(h(ListDetail, { list: "Rows", detail: h("p", null, "Fix it"), listWidth: 200, onBack: () => backs++ }));
  assert.ok(host.querySelector(".w-list-detail").hasAttribute("data-has-detail"));
  assert.equal(host.querySelector(".w-list-detail").style.getPropertyValue("--w-list-width"), "200px");
  assert.ok(host.querySelector(".w-list-detail-detail").textContent.includes("Fix it"));
  await click(host.querySelector(".w-list-detail-back"));
  assert.equal(backs, 1);
  await unmount();
});

test("Board is a focusable region, and a BoardColumn is named by its title and counts its cards", async () => {
  const { host, unmount } = await mount(h(Board, { label: "Sprint" }, h(BoardColumn, { title: "Doing", count: 2 }, h("p", null, "Card"))));
  const board = host.querySelector("[role=region]");
  assert.equal(board.getAttribute("aria-label"), "Sprint");
  assert.equal(board.getAttribute("tabindex"), "0");
  const column = host.querySelector("section");
  assert.equal(document.getElementById(column.getAttribute("aria-labelledby")).textContent, "Doing");
  assert.equal(column.querySelector(".w-board-column-count").textContent, "2");
  await unmount();
});

test("BoardLanes heads the columns once, and a BoardLane folds its cells with a button that says whether it's open", async () => {
  const lane = (props) => h(BoardLane, { label: "WEB-10 Checkout", count: 2, ...props }, h(BoardCell, { label: "To do" }, h("p", null, "Card")), h(BoardCell, { label: "Done" }));
  const { host, unmount } = await mount(h(BoardLanes, { label: "Issues by parent", columns: [{ id: "todo", title: "To do", count: 2 }, { id: "done", title: "Done" }] }, lane({})));
  const lanes = host.querySelector(".w-board-lanes");
  assert.equal(lanes.style.getPropertyValue("--w-board-columns"), "2");
  assert.deepEqual([...host.querySelectorAll(".w-board-lanes-column-title")].map((c) => c.textContent), ["To do", "Done"]);
  const region = host.querySelector("[role=region]");
  assert.equal(region.getAttribute("aria-label"), "Issues by parent");
  assert.equal(region.getAttribute("tabindex"), "0");
  const section = region.querySelector("section");
  assert.equal(document.getElementById(section.getAttribute("aria-labelledby")).textContent, "WEB-10 Checkout");
  assert.deepEqual([...section.querySelectorAll("[role=group]")].map((g) => g.getAttribute("aria-label")), ["To do", "Done"]);
  const toggle = section.querySelector(".w-board-lane-toggle");
  const body = document.getElementById(toggle.getAttribute("aria-controls"));
  assert.equal(toggle.getAttribute("aria-label"), "WEB-10 Checkout");
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  assert.equal(body.hidden, false);
  await click(toggle);
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(body.hidden, true);
  assert.equal(section.hasAttribute("data-open"), false);
  await unmount();

  // Controlled: it shows what `open` says and asks `onOpenChange` to change it.
  const asked = [];
  const controlled = await mount(h(BoardLanes, { label: "Lanes", columns: [] }, lane({ open: false, onOpenChange: (open) => asked.push(open) })));
  const button = controlled.host.querySelector(".w-board-lane-toggle");
  await click(button);
  assert.deepEqual(asked, [true]);
  assert.equal(button.getAttribute("aria-expanded"), "false");
  await controlled.unmount();
});

test("Markdown loads a remote image only after its button is clicked", async () => {
  const { host, unmount } = await mount(h(Markdown, { source: "![chart](https://example.com/c.png)" }));
  assert.equal(host.querySelector("img"), null);
  await click(host.querySelector("button.w-md-image-load"));
  const img = host.querySelector("img");
  assert.equal(img.getAttribute("src"), "https://example.com/c.png");
  assert.equal(host.querySelector("button.w-md-image-load"), null);
  await unmount();
});

test("TimelineCard with clamp offers Show more only when the body was cut, and toggles it", async () => {
  const sizes = { scrollHeight: 400, clientHeight: 100 };
  for (const key of Object.keys(sizes)) Object.defineProperty(dom.window.HTMLElement.prototype, key, { configurable: true, get: () => sizes[key] });
  const { host, unmount } = await mount(h(TimelineCard, { author: "Ada", at: "2026-10-13T11:55:00Z", clamp: true }, h("p", null, "long")));
  const box = host.querySelector(".w-clamp-box");
  assert.equal(box.hasAttribute("data-clamped"), true);
  const toggle = host.querySelector(".w-clamp-toggle");
  assert.equal(toggle.textContent, "Show more");
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  await click(toggle);
  assert.equal(host.querySelector(".w-clamp-box").hasAttribute("data-clamped"), false);
  assert.equal(host.querySelector(".w-clamp-toggle").textContent, "Show less");
  await unmount();
  sizes.scrollHeight = 50;
  const short = await mount(h(TimelineCard, { author: "Ada", clamp: true }, h("p", null, "short")));
  assert.equal(short.host.querySelector(".w-clamp-toggle"), null);
  await short.unmount();
  for (const key of Object.keys(sizes)) delete dom.window.HTMLElement.prototype[key];
});

test("a clamped body makes what the cut hides inert until Show more", async () => {
  const sizes = { scrollHeight: 400, clientHeight: 100 };
  for (const key of Object.keys(sizes)) Object.defineProperty(dom.window.HTMLElement.prototype, key, { configurable: true, get: () => sizes[key] });
  // jsdom has no layout: the box ends at 100, the first link sits above that, the second below.
  const top = (el) => (el.dataset.at ? Number(el.dataset.at) : 0);
  dom.window.HTMLElement.prototype.getBoundingClientRect = function () { return { top: top(this), bottom: this.classList.contains("w-clamp-box") ? 100 : top(this) + 20 }; };
  const body = h("div", null, h("a", { href: "#a", "data-at": 10, id: "a" }, "a"), h("a", { href: "#b", "data-at": 300, id: "b" }, "b"));
  const { host, unmount } = await mount(h(TimelineCard, { author: "Ada", clamp: true }, body));
  assert.equal(host.querySelector("#a").inert, false);
  assert.equal(host.querySelector("#b").inert, true);
  await click(host.querySelector(".w-clamp-toggle"));
  assert.equal(host.querySelector("#b").inert, false);
  await unmount();
  delete dom.window.HTMLElement.prototype.getBoundingClientRect;
  for (const key of Object.keys(sizes)) delete dom.window.HTMLElement.prototype[key];
});

// ---------- page hooks: each test stands in for `wings` with what the page asks of it ----------

/** A promise that settles when the test says so. */
const later = () => { let on; const promise = new Promise((resolve, reject) => (on = { resolve, reject })); return { promise, ...on }; };

test("ProjectGate shows Loading, then the page keyed by the project's id, and an Empty when there is none", async () => {
  let tell;
  let mounts = 0;
  globalThis.wings = { onProject: (listener) => ((tell = listener), () => {}) };
  const Page = ({ project }) => (useEffect(() => void mounts++, []), h("p", null, project.branch));
  const { host, unmount } = await mount(h(ProjectGate, { loading: "Loading issues", empty: { description: "Open one." } }, (project) => h(Page, { project })));
  assert.equal(host.textContent, "Loading issues");
  await act(() => tell({ id: "a", branch: "main" }));
  await act(() => tell({ id: "a", branch: "dev" }));
  assert.deepEqual([host.textContent, mounts], ["dev", 1], "a branch change keeps the page");
  await act(() => tell({ id: "b", branch: "main" }));
  assert.equal(mounts, 2, "another project starts the page over");
  await act(() => tell(null));
  assert.deepEqual([...host.querySelectorAll("p")].map((p) => p.textContent), ["No project", "Open one."]);
  await unmount();
});

test("showBlock mounts the page in the view shell and tells Wings the shell's height, rounded up, as it changes", async () => {
  const heights = [];
  let notify;
  let height = 200.4;
  globalThis.wings = { setHeight: (px) => heights.push(px) };
  globalThis.ResizeObserver = class { constructor(callback) { notify = callback; } observe() {} };
  const shell = Object.assign(document.body.appendChild(document.createElement("main")), { id: "app", getBoundingClientRect: () => ({ height }) });
  await act(() => showBlock(h("p", null, "Issues")));
  assert.deepEqual([shell.dataset.place, shell.textContent], ["view", "Issues"]);
  notify();
  height = 310;
  notify();
  assert.deepEqual(heights, [201, 310]);
  delete globalThis.ResizeObserver;
  shell.remove();
});

/** A page that shows the `size` of its saved state, which a click sets to 300. */
const sizePage = (hook) =>
  function Page() {
    const [state, set] = hook({ size: 280 }, (raw) => (typeof raw?.size === "number" ? raw : undefined));
    return h("button", { onClick: () => set({ size: 300 }) }, state ? String(state.size) : "…");
  };

test("useTabState and useBlockState are null until the state is read, then what check makes of it or the initial one, and save what is set", async () => {
  for (const [hook, get, put] of [[useTabState, "tabState", "setTabState"], [useBlockState, "blockState", "setBlockState"]]) {
    for (const [answer, shown] of [[(r) => r.resolve({ size: 120 }), "120"], [(r) => r.resolve({ size: "wide" }), "280"], [(r) => r.reject(new Error("gone")), "280"]]) {
      const [read, saved] = [later(), []];
      globalThis.wings = { [get]: () => read.promise, [put]: (state) => (saved.push(state), Promise.reject(new Error("too big"))) };
      const { host, unmount } = await mount(h(sizePage(hook)));
      assert.equal(host.textContent, "…");
      answer(read);
      await settle();
      assert.equal(host.textContent, shown);
      await act(() => host.querySelector("button").click());
      assert.deepEqual([host.textContent, saved], ["300", [{ size: 300 }]], "a save that is refused is kept");
      await unmount();
    }
  }
});

test("useViewArgs gives the args as check reads them, or null, and reads them once per page", async () => {
  let [seen, reads] = [undefined, 0];
  const Page = () => ((seen = useViewArgs((raw) => (typeof raw.id === "number" ? { id: raw.id } : null))), null);
  for (const [raw, expected] of [[{ id: 7, extra: 1 }, { id: 7 }], [{ id: "7" }, null], [null, null]]) {
    globalThis.wings = { viewArgs: () => (reads++, raw) };
    const { render, unmount } = await mount(h(Page));
    await render(h(Page));
    assert.deepEqual(seen, expected);
    await unmount();
  }
  assert.equal(reads, 3);
});

test("copyText writes to the clipboard, and uses a textarea and execCommand when the clipboard refuses", async () => {
  const [written, copies] = [[], []];
  const clipboard = (writeText) => Object.defineProperty(dom.window.navigator, "clipboard", { configurable: true, value: { writeText } });
  dom.window.document.execCommand = (command) => copies.push([command, document.querySelector("textarea").value]);
  clipboard(async (text) => void written.push(text));
  await copyText("fix/login");
  clipboard(() => Promise.reject(new Error("denied")));
  await copyText("fix/login");
  assert.deepEqual([written, copies, document.querySelector("textarea")], [["fix/login"], [["copy", "fix/login"]], null]);
  delete dom.window.navigator.clipboard;
  delete dom.window.document.execCommand;
});

test("useAction is busy while any run is going, and keeps the message of the last one that threw until the next run starts", async () => {
  let action;
  const Page = () => ((action = useAction()), null);
  const { unmount } = await mount(h(Page));
  const [one, two, runs] = [later(), later(), []];
  await act(async () => void runs.push(action.run(() => one.promise), action.run(() => two.promise)));
  await act(async () => { one.resolve(); await runs[0]; });
  assert.equal(action.busy, true, "another run is still going");
  await act(async () => { two.reject(new Error("Disk full")); await runs[1]; });
  assert.deepEqual([action.busy, action.error], [false, "Disk full"]);
  await act(() => action.run(async () => {}));
  assert.equal(action.error, null, "a new run clears it");
  await unmount();
});

test("useAction's run resolves to whether the action worked, so a caller can keep a draft when it didn't", async () => {
  let action;
  const Page = () => ((action = useAction()), null);
  const { unmount } = await mount(h(Page));
  let worked, failed;
  await act(async () => {
    worked = await action.run(async () => {});
    failed = await action.run(() => Promise.reject(new Error("Disk full")));
  });
  assert.deepEqual([worked, failed], [true, false]);
  await unmount();
});

test("usePoll loads at once and when load changes, then every interval while Wings is focused, and stops on unmount", async (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  let focus;
  globalThis.wings = { focused: true, onFocus: (listener) => ((focus = listener), () => (focus = null)) };
  const calls = [];
  const Page = ({ name }) => (usePoll(useCallback(() => calls.push(name), [name]), 1000), null);
  const { render, unmount } = await mount(h(Page, { name: "a" }));
  t.mock.timers.tick(2000);
  focus({ focused: false });
  t.mock.timers.tick(5000);
  focus({ focused: true });
  await render(h(Page, { name: "b" }));
  await unmount();
  t.mock.timers.tick(5000);
  assert.deepEqual(calls, ["a", "a", "a", "a", "b"], "at once, each interval, once Wings is back, and when load changes; not while away or after unmount");
  assert.equal(focus, null, "the focus listener is removed");
});
