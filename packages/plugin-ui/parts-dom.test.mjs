// node --test: the parts that keep what you typed when a save fails, in jsdom. Its own page setup, like ui-dom.test.mjs.
import assert from "node:assert/strict";
import { after, test } from "node:test";
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><body></body>", { pretendToBeVisual: true });
after(() => dom.window.close());
for (const key of Object.getOwnPropertyNames(dom.window)) {
  if (!(key in globalThis) || key === "navigator") Object.defineProperty(globalThis, key, { configurable: true, get: () => dom.window[key] });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
dom.window.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };
const { act, createElement: h } = await import("react");
const { createRoot } = await import("react-dom/client");
const { Confirm, Field, InlineInput, Textarea } = await import("./ui.js");

async function mount(element) {
  const host = document.body.appendChild(document.createElement("div"));
  const root = createRoot(host);
  await act(() => root.render(element));
  return { host, unmount: () => act(() => root.unmount()) };
}

test("Textarea in a Field is a textarea named by the field's label", async () => {
  const { host, unmount } = await mount(h(Field, { label: "Notes" }, h(Textarea, { rows: 2, defaultValue: "hi" })));
  const area = host.querySelector("textarea");
  assert.ok(area?.classList.contains("w-textarea"));
  assert.equal(area.rows, 2);
  assert.equal(host.querySelector("label").htmlFor, area.id);
  await unmount();
});

const type = (input, text) => act(() => {
  Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, "value").set.call(input, text);
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
});
const key = (el, k) => act(() => { el.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); });
const blur = (el) => act(() => { el.blur(); });
const later = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

test("InlineInput stays open with its text when the save fails, shows why, and a second Enter saves", async () => {
  const saved = [];
  let done = 0;
  let fail = true;
  const onSave = async (v) => { if (fail) throw new Error("Disk is full"); saved.push(v); };
  const { host, unmount } = await mount(h(InlineInput, { label: "Title", value: "Old", onSave, onDone: () => done++ }));
  const input = host.querySelector("input");
  await type(input, " New title ");
  await key(input, "Enter");
  await later();
  assert.equal(done, 0);
  assert.equal(input.value, " New title ");
  assert.equal(host.querySelector("[role=alert]").textContent, "Disk is full");
  assert.equal(input.getAttribute("aria-invalid"), "true");
  assert.equal(input.getAttribute("aria-describedby"), host.querySelector("[role=alert]").id);
  await type(input, "New title");
  assert.equal(host.querySelector("[role=alert]"), null, "typing clears the error");
  fail = false;
  await key(input, "Enter");
  await later();
  assert.deepEqual(saved, ["New title"]);
  assert.equal(done, 1);
  await unmount();
});

test("InlineInput ignores Escape and blur while a save is out, and cancels on Escape otherwise", async () => {
  let release;
  let saves = 0;
  let done = 0;
  const onSave = () => new Promise((r) => { saves++; release = r; });
  const { host, unmount } = await mount(h(InlineInput, { label: "Title", value: "Old", onSave, onDone: () => done++ }));
  const input = host.querySelector("input");
  await type(input, "New");
  await key(input, "Enter");
  assert.equal(input.readOnly, true);
  await key(input, "Escape");
  await blur(input);
  assert.equal(saves, 1);
  assert.equal(done, 0);
  await act(async () => release());
  await later();
  assert.equal(done, 1);
  await unmount();

  done = 0;
  const again = await mount(h(InlineInput, { label: "Title", value: "Old", onSave, onDone: () => done++ }));
  const field = again.host.querySelector("input");
  await type(field, "Other");
  await key(field, "Escape");
  assert.equal(done, 1);
  assert.equal(saves, 1, "Escape does not save");
  await again.unmount();
});

const click = (el) => act(() => { el.click(); });

test("Confirm stays open and shows why when an action fails, then closes when the page closes it", async () => {
  let fail = true;
  let open = true;
  const onConfirm = async () => { if (fail) throw new Error("Not allowed"); open = false; };
  const el = () => h(Confirm, { title: "Delete it?", onCancel() {}, actions: [{ id: "go", label: "Delete", onConfirm }] }, "This is final.");
  const { host, unmount } = await mount(el());
  const card = host.querySelector("[role=alertdialog]");
  assert.equal(document.activeElement.textContent, "Cancel");
  await click([...host.querySelectorAll("button")].find((b) => b.textContent === "Delete"));
  await later();
  assert.ok(host.querySelector("[role=alertdialog]"), "still open");
  assert.equal(host.querySelector("[role=alert]").textContent, "Not allowed");
  assert.equal(card.getAttribute("aria-describedby"), host.querySelector("[role=alert]").id);
  fail = false;
  await click([...host.querySelectorAll("button")].find((b) => b.textContent === "Delete"));
  await later();
  assert.equal(open, false);
  assert.equal(host.querySelector("[role=alert]"), null, "the error is gone with the next try");
  await unmount();
});

test("Confirm cancels on Escape unless an action is running, and focuses the action that asks to be", async () => {
  let cancels = 0;
  let release;
  const onConfirm = () => new Promise((r) => { release = r; });
  const { host, unmount } = await mount(h(Confirm, { title: "Delete?", onCancel: () => cancels++, actions: [{ id: "go", label: "Delete", onConfirm, autoFocus: true }] }));
  assert.equal(document.activeElement.textContent, "Delete");
  const card = host.querySelector("[role=alertdialog]");
  await click(document.activeElement);
  await key(card, "Escape");
  assert.equal(cancels, 0);
  await act(async () => release());
  await later();
  await key(card, "Escape");
  assert.equal(cancels, 1);
  await unmount();
});
