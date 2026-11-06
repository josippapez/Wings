import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const find = (node, kind) => node.type === kind ? node : node.children?.map((n) => find(n, kind)).find(Boolean);
const until = async (read) => {
  const deadline = Date.now() + 3000;
  while (!read()) {
    assert.ok(Date.now() < deadline, "native page did not finish rendering");
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
};

function frame(t, name, answer) {
  const trees = [], timers = new Set(), asks = [];
  let listener;
  const emit = (m) => listener?.({ "wings:sdk": 1, ...m });
  const context = {
    Intl: undefined, queueMicrotask,
    setTimeout: (fn, ms) => { const id = setTimeout(fn, ms); timers.add(id); return id; },
    clearTimeout: (id) => { clearTimeout(id); timers.delete(id); },
    setInterval: (fn, ms) => { const id = setInterval(fn, ms); timers.add(id); return id; },
    clearInterval: (id) => { clearInterval(id); timers.delete(id); },
    wings: {
      focused: true,
      render: (json) => trees.push(JSON.parse(json)),
      onFocus: () => () => {},
      onBroadcast: (fn) => { listener = fn; return () => {}; },
      broadcast: async (m) => {
        if (m.kind === "ask") { asks.push(m); emit({ kind: "answer", id: m.id, response: answer(m) }); }
        if (m.kind === "sub") for (const [name, key] of m.subs) emit({ kind: "state", name, key, data: answer({ name, key }), at: Date.now() });
      },
      exec: async () => ({ code: 0, stdout: log, stderr: "" }),
    },
  };
  t.after(() => { for (const timer of timers) clearTimeout(timer); });
  const source = readFileSync(new URL(`../../dist/native/${name}.js`, import.meta.url), "utf8");
  assert.doesNotMatch(source, /getComputedStyle|ResizeObserver|document\./);
  runInNewContext(source, context);
  return { context, trees, asks, last: () => trees.at(-1) };
}

const hash = (i) => i.toString(16).padStart(40, "0");
const log = Array.from({ length: 10000 }, (_, i) => [hash(i), i === 0 ? `${hash(1)} ${hash(2)}` : i === 1 || i === 2 ? hash(3) : i < 9999 ? hash(i + 1) : "", "Ada", 1770000000, 1770000000, i === 0 ? "HEAD -> refs/heads/main" : "", `Commit ${i}`].join("\0")).join("\0") + "\0";

test("built native graph runs without DOM or Intl and opens requested commits from a bounded window", async (t) => {
  let currentRoot = "/fixture/web";
  const f = frame(t, "graph", (m) => m.name === "graph-repo" ? { root: currentRoot, name: "web" } : null);
  await until(() => f.last() && find(f.last(), "Graph"));
  let graph = find(f.last(), "Graph");
  assert.equal(graph.props.total, 10000);
  assert.equal(graph.props.rows.length, 64);
  assert.equal(graph.children.length, 0);
  assert.equal(graph.props.rows[0].merge, true);
  assert.deepEqual(graph.props.rows[0].refs, ["H:main"]);
  assert.ok(graph.props.rows.some((row) => row.lane === 1));
  f.context.__wingsEvent(graph.props.onRangeChange, '{"start":9000,"end":9070}');
  await until(() => find(f.last(), "Graph").props.offset === 9000);
  graph = find(f.last(), "Graph");
  assert.equal(graph.props.rows.length, 70);
  assert.equal(graph.props.rows[0].hash, hash(9000));
  assert.ok(JSON.stringify(f.last()).length < 50000);
  const before = f.trees.length;
  f.context.__wingsEvent(graph.props.onRangeChange, '{"start":9000,"end":9070}');
  assert.equal(f.trees.length, before);
  f.context.__wingsEvent(graph.props.onSelect, '{"index":9010}');
  await until(() => f.asks.some((m) => m.name === "commit"));
  const commit = f.asks.find((m) => m.name === "commit").request;
  assert.equal(commit.root, "/fixture/web");
  assert.equal(commit.hash, hash(9010));
  assert.equal(commit.subject, "Commit 9010");
  let loaded;
  f.context.wings.exec = () => new Promise((resolve) => { loaded = resolve; });
  currentRoot = "/fixture/other";
  f.context.__wingsEvent(find(f.last(), "Button").props.onClick, "");
  await until(() => loaded && !find(f.last(), "Graph"));
  loaded({ code: 0, stdout: log, stderr: "" });
  await until(() => find(f.last(), "Graph"));
  assert.equal(find(f.last(), "Graph").props.offset, 0);
  assert.equal(find(f.last(), "Graph").props.selected, -1);
});

test("built native sidebar sends typed history search to its query", async (t) => {
  const keys = [];
  const repo = { root: "/fixture/web", name: "web", branch: "main" };
  const f = frame(t, "sidebar", (m) => {
    if (m.name === "changes") return { repo, files: [], total: 0, added: 0, removed: 0, against: "HEAD" };
    if (m.name === "history") { keys.push(m.key); return { repo, rows: [], more: false }; }
    return null;
  });
  await until(() => f.last() && find(f.last(), "Chips"));
  f.context.__wingsEvent(find(f.last(), "Chips").props.onChange, '{"value":"history"}');
  await until(() => find(f.last(), "SearchField"));
  f.context.__wingsEvent(find(f.last(), "SearchField").props.onInput, '{"value":" coupon "}');
  await until(() => keys.some((key) => key.query === "coupon"));
  assert.equal(keys.at(-1).all, false);
});
