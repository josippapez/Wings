import assert from "node:assert/strict";
import { mock, test } from "node:test";

import { relay, settle } from "./fake-frames.mjs";
import { channel } from "./messages.js";

/** Three frames of one plugin: main, and two pages. */
function plugin() {
  const frame = relay();
  const frames = [frame(), frame(), frame()];
  const [main, a, b] = frames.map((f) => channel({ wings: f.wings }));
  return { main, a, b, frames };
}

test("an ask gets the answer, and an error in the handler reaches the asker with its message", async () => {
  const { main, a } = plugin();
  main.answer("double", (n) => n * 2);
  main.answer("fail", () => {
    throw new Error("No issue WIN-9");
  });
  main.answer("reject", async () => {
    throw "plain text";
  });
  assert.equal(await a.ask("double", 21), 42);
  await assert.rejects(a.ask("fail"), { name: "Error", message: "No issue WIN-9" });
  await assert.rejects(a.ask("reject"), { message: "plain text" });
});

test("two pages asking at once each get their own answer, though both hear both", async () => {
  const { main, a, b } = plugin();
  main.answer("echo", async ({ word, ms }) => {
    await new Promise((resolve) => setTimeout(resolve, ms));
    return word;
  });
  const answers = await Promise.all([a.ask("echo", { word: "slow", ms: 30 }), b.ask("echo", { word: "fast", ms: 1 }), a.ask("echo", { word: "same page", ms: 10 })]);
  assert.deepEqual(answers, ["slow", "fast", "same page"]);
});

test("an ask nobody answers rejects when its timeout is up", async () => {
  const { a } = plugin();
  await assert.rejects(a.ask("nobody", null, { timeoutMs: 20 }), { message: "Wings didn't answer. Try again." });
});

test("a tell reaches the other frames' listeners and not the sender's, until the listener is removed", async () => {
  const { main, a, b } = plugin();
  const heard = [];
  const off = b.on("moved", (n) => heard.push(["b", n]));
  a.on("moved", (n) => heard.push(["a", n]));
  main.on("moved", (n) => heard.push(["main", n]));
  await a.tell("moved", 1);
  await settle();
  assert.deepEqual(heard.sort(), [["b", 1], ["main", 1]]);
  off();
  await a.tell("moved", 2);
  await settle();
  assert.deepEqual(heard.sort(), [["b", 1], ["main", 1], ["main", 2]]);
});

test("broadcasts that aren't the SDK's own are left alone", async () => {
  const { main, frames } = plugin();
  main.answer("double", (n) => n * 2);
  await settle();
  const sent = frames[0].sent.length;
  await frames[1].wings.broadcast({ kind: "ask", name: "double", id: "x", request: 1 });
  await settle();
  assert.equal(frames[0].sent.length, sent);
});

// ---------- queries ----------

/** Lets every message in flight land, parts and the loads they start included. */
const drain = async () => {
  for (let i = 0; i < 20; i++) await settle();
};

/** Main and two pages on mock clocks, the keys main loaded, and `values` to record what a page's subscription hears. */
function queries(t) {
  mock.timers.enable({ apis: ["setTimeout", "setInterval", "Date"], now: 1_000_000 });
  t.after(() => mock.timers.reset());
  const make = relay();
  const frames = [make(), make(), make()];
  const [main, a, b] = frames.map((f) => channel({ wings: f.wings }));
  const loads = [];
  const values = (page, name, key) => {
    const seen = [];
    page.subscribe(name, key, (s) => !s.loading && seen.push(s.error ?? s.data));
    return seen;
  };
  return { make, frames, main, a, b, loads, values };
}

const ticks = async (n, ms) => {
  for (let i = 0; i < n; i++) {
    mock.timers.tick(ms);
    await drain();
  }
};

test("two pages on one key share one load, keys don't mix, and key order doesn't matter", async (t) => {
  const { main, a, b, loads, values } = queries(t);
  main.serve("pr", (key) => (loads.push(key), `pr ${key.n} of ${key.repo}`));
  const one = values(a, "pr", { repo: "web", n: 1 });
  const same = values(b, "pr", { n: 1, repo: "web" });
  const two = values(b, "pr", { repo: "web", n: 2 });
  await drain();
  assert.deepEqual(loads, [{ repo: "web", n: 1 }, { repo: "web", n: 2 }]);
  assert.deepEqual([one, same, two], [["pr 1 of web"], ["pr 1 of web"], ["pr 2 of web"]]);
  assert.equal(await main.get("pr", { n: 1, repo: "web" }, { maxAgeMs: 60_000 }), "pr 1 of web");
  assert.equal(loads.length, 2);
});

test("a put reaches main and the other page in one broadcast, and a refresh from one page updates both after one load", async (t) => {
  const { main, a, b, frames, loads, values } = queries(t);
  let n = 0;
  main.serve("count", () => (loads.push(1), ++n));
  const seenA = values(a, "count");
  const seenB = values(b, "count");
  await drain();
  const sent = frames[1].sent.length;
  a.put("count", undefined, 10);
  await drain();
  assert.equal(frames[1].sent.length, sent + 1);
  assert.equal(main.peek("count")?.data, 10);
  b.refresh("count");
  await drain();
  assert.equal(loads.length, 2);
  assert.deepEqual([seenA, seenB], [[1, 10, 2], [1, 10, 2]]);
});

test("a 1 MB state, a 200 KB put and a 70 KB answer arrive whole, in parts under the limit, escapes and emoji intact", async (t) => {
  const { main, a, b, frames, values } = queries(t);
  const big = '"\\\n😀'.repeat(150_000);
  main.serve("big", () => big);
  main.answer("seventy", () => "😀".repeat(35_000));
  const seenA = values(a, "big");
  const other = values(b, "small");
  await drain();
  assert.equal(seenA[0], big);
  assert.deepEqual(other, []);
  b.put("big", undefined, 'x"😀'.repeat(50_000));
  await drain();
  assert.equal(seenA[1], 'x"😀'.repeat(50_000));
  assert.equal(await a.ask("seventy"), "😀".repeat(35_000));
  for (const f of frames) for (const m of f.sent) assert.ok(JSON.stringify(m).length <= 64_000);
});

test("a failed load keeps the value and reaches every page showing the key, and the next good load clears it", async (t) => {
  const { main, a, b, values } = queries(t);
  let fail = false;
  main.serve("pulls", () => {
    if (fail) throw new Error("gh isn't signed in");
    return ["#1"];
  });
  const seenA = values(a, "pulls");
  const seenB = values(b, "pulls");
  await drain();
  fail = true;
  main.refresh("pulls");
  await drain();
  fail = false;
  a.refresh("pulls");
  await drain();
  const expected = [["#1"], "gh isn't signed in", ["#1"]];
  assert.deepEqual([seenA, seenB], [expected, expected]);
});

test("main refetches while a page shows the key and Wings is focused, catches up on focus, and stops when the page's lease runs out", async (t) => {
  const { make, frames, main, a, loads } = queries(t);
  main.serve("pr", () => loads.push(1), { refetchMs: 60_000, staleMs: 55_000 });
  a.subscribe("pr", undefined, () => {});
  await drain();
  await ticks(1, 60_000);
  assert.equal(loads.length, 2);
  make.focus(false);
  await ticks(2, 60_000);
  assert.equal(loads.length, 2);
  make.focus(true);
  await drain();
  assert.equal(loads.length, 3);
  // The page goes away without a word: its lease, renewed on focus, runs out 90 s later and the refetches stop.
  frames[1].close();
  await ticks(5, 60_000);
  assert.equal(loads.length, 4);
});

test("a page that subscribes before main, and a main that restarts, get the value without a change", async (t) => {
  const { make, frames, main, a, values } = queries(t);
  const seen = values(a, "hud");
  await drain();
  main.serve("hud", () => "first");
  await drain();
  frames[0].close();
  const again = channel({ wings: make().wings });
  again.serve("hud", () => "second");
  await drain();
  assert.deepEqual(seen, ["first", "second"]);
});
