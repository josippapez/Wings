/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { parseViewLabel, viewLabel, ViewWindowHost, ViewWindowRelay, type Bus, type ViewDocument } from "./plugin-views.ts";

/** Windows as Tauri delivers to them: `emitTo` reaches one label, `emit` all, and payloads go through JSON. */
function network() {
  const windows = new Map<string, Map<string, ((payload: unknown) => void)[]>>();
  const deliver = (label: string, event: string, payload: unknown) => {
    for (const handler of [...(windows.get(label)?.get(event) ?? [])]) handler(JSON.parse(JSON.stringify(payload ?? null)));
  };
  const bus = (self: string): Bus => ({
    emitTo: (label, event, payload) => deliver(label, event, payload),
    emit: (event, payload) => [...windows.keys()].forEach((label) => deliver(label, event, payload)),
    listen: async <T,>(event: string, handler: (payload: T) => void) => {
      const events = windows.get(self) ?? new Map<string, ((payload: unknown) => void)[]>();
      windows.set(self, events);
      const list = events.get(event) ?? [];
      events.set(event, list);
      const h = handler as (payload: unknown) => void;
      list.push(h);
      return () => void list.splice(list.indexOf(h), 1);
    },
  });
  return { bus, close: (label: string) => windows.delete(label) };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A stand-in for the frame's window: records what the relay posts to it. */
function frame() {
  const got: unknown[] = [];
  return { got, postMessage: (message: unknown) => void got.push(message) };
}

/** A host that attaches `known` labels and records what reaches it, and one view window. */
async function setup(known: string[], label = viewLabel("demo", "board")) {
  const net = network();
  const log = { messages: [] as [string, unknown][], detached: [] as string[], asked: [] as string[] };
  const host = new ViewWindowHost(net.bus("main"), {
    attach: async (l) => (log.asked.push(l), known.includes(l) ? { srcdoc: `<p>${l}</p>`, title: "Board" } : null),
    message: (l, data) => void log.messages.push([l, data]),
    detach: (l) => void log.detached.push(l),
  });
  await settle();
  const page = frame();
  const mounted: ViewDocument[] = [];
  const relay = new ViewWindowRelay(label, net.bus(label), (doc) => (mounted.push(doc), page));
  await settle();
  return { net, host, relay, page, mounted, log, label };
}

test("a view window gets its page, and its frame's calls and the host's answers go both ways", async () => {
  const { host, relay, page, mounted, log, label } = await setup([viewLabel("demo", "board")]);
  assert.deepEqual(mounted, [{ srcdoc: `<p>${label}</p>`, title: "Board" }]);
  relay.loaded();
  relay.receive(page, { wings: 1, nonce: "n", id: 1, method: "exec", params: { program: "git" } });
  assert.deepEqual(log.messages, [[label, { wings: 1, nonce: "n", id: 1, method: "exec", params: { program: "git" } }]]);
  host.post(label, { wings: 1, id: 1, result: { code: 0 } });
  host.post(label, { wings: 1, event: "broadcast", data: { n: 2 } });
  assert.deepEqual(page.got, [
    { wings: 1, id: 1, result: { code: 0 } },
    { wings: 1, event: "broadcast", data: { n: 2 } },
  ]);
});

test("only the frame's own messages reach the host", async () => {
  const { relay, log } = await setup([viewLabel("demo", "board")]);
  relay.loaded();
  relay.receive(frame(), { wings: 1, id: 1, method: "exec" });
  relay.receive(null, { wings: 1, id: 2, method: "exec" });
  assert.deepEqual(log.messages, []);
});

test("a window nobody opened gets no page", async () => {
  const { mounted, log, label } = await setup([]);
  assert.deepEqual(log.asked, [label]);
  assert.deepEqual(mounted, []);
});

test("a frame that navigates away is let go and can't speak for the plugin", async () => {
  const { relay, page, log, label } = await setup([viewLabel("demo", "board")]);
  relay.loaded();
  relay.loaded();
  assert.deepEqual(log.detached, [label]);
  relay.receive(page, { wings: 1, id: 1, method: "exec" });
  assert.deepEqual(log.messages, []);
});

test("a closed window is detached", async () => {
  const { net, log, label } = await setup([viewLabel("demo", "board")]);
  // Rust sends this to the main window when the view window is destroyed.
  net.bus("rust").emitTo("main", "plugin-view:closed", label);
  assert.deepEqual(log.detached, [label]);
});

test("a host that starts again asks open windows for hello, so they get a fresh page", async () => {
  const { net, mounted, label } = await setup([viewLabel("demo", "board")]);
  const asked: string[] = [];
  new ViewWindowHost(net.bus("main"), {
    attach: async (l) => (asked.push(l), { srcdoc: "<p>again</p>", title: "Board" }),
    message: () => {},
    detach: () => {},
  });
  await settle();
  await settle();
  assert.ok(asked.includes(label));
  assert.equal(mounted.at(-1)?.srcdoc, "<p>again</p>");
});

test("a host's messages for one view window don't reach another", async () => {
  const { net, host, page } = await setup([viewLabel("demo", "board"), viewLabel("demo", "chart")]);
  const other = frame();
  new ViewWindowRelay(viewLabel("demo", "chart"), net.bus(viewLabel("demo", "chart")), () => other);
  await settle();
  await settle();
  host.post(viewLabel("demo", "chart"), { wings: 1, id: 7, result: null });
  assert.deepEqual(other.got, [{ wings: 1, id: 7, result: null }]);
  assert.deepEqual(page.got, []);
});

test("view labels name one view of one plugin", () => {
  assert.equal(viewLabel("time-tracker", "board"), "view:time-tracker:board");
  assert.deepEqual(parseViewLabel("view:time-tracker:board"), { pluginId: "time-tracker", viewId: "board" });
  for (const label of ["main", "panel-a-b", "view:a", "view:a:b:c", "view::b"]) assert.equal(parseViewLabel(label), null, label);
});
