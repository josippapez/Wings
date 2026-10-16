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
const { Field, Textarea } = await import("./ui.js");

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
