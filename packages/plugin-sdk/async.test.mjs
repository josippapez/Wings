import assert from "node:assert/strict";
import { test } from "node:test";
import { setImmediate as next, setTimeout as sleep } from "node:timers/promises";

import { cached, errorMessage, fitJson, latestOnly, mapLimit, openDiffWhile, serial, singleFlight } from "./async.js";

test("errorMessage takes the message of an Error or of anything with one, else the text of the value", () => {
  assert.equal(errorMessage(new Error("boom")), "boom");
  assert.equal(errorMessage({ message: "from the bridge" }), "from the bridge");
  assert.equal(errorMessage("plain"), "plain");
});

test("cached shares a running load, keeps a value for the ttl, and loads again after it", async (t) => {
  t.mock.timers.enable({ apis: ["Date"] });
  let loads = 0;
  const c = cached(async (k) => `${k}${++loads}`, { ttlMs: 1000 });
  assert.deepEqual(await Promise.all([c.get("a"), c.get("a")]), ["a1", "a1"]);
  t.mock.timers.tick(999);
  assert.equal(await c.get("a"), "a1");
  t.mock.timers.tick(1);
  assert.equal(c.peek("a"), "a1", "peek gives a value of any age");
  assert.equal(await c.get("a"), "a2");
  assert.equal(await c.get("a", { fresh: true }), "a3");
  assert.equal(await c.get("b"), "b4", "keys are separate");
});

test("cached forgets a failure, unless failTtlMs asks to remember it for a while", async (t) => {
  t.mock.timers.enable({ apis: ["Date"] });
  let n = 0;
  const flaky = cached(async () => {
    if (++n < 3) throw new Error(`fail ${n}`);
    return "ok";
  });
  await assert.rejects(flaky.get("k"), /fail 1/);
  await assert.rejects(flaky.get("k"), /fail 2/);
  assert.equal(await flaky.get("k"), "ok");

  let m = 0;
  const down = cached(async () => Promise.reject(new Error(`down ${++m}`)), { failTtlMs: 500 });
  await assert.rejects(down.get("k"), /down 1/);
  await assert.rejects(down.get("k"), /down 1/);
  t.mock.timers.tick(500);
  await assert.rejects(down.get("k"), /down 2/);
});

test("cached takes an object key through key(), and delete and clear drop what was kept", async () => {
  let loads = 0;
  const c = cached(({ root, mode }) => `${root}:${mode}:${++loads}`, { key: (k) => `${k.root}\0${k.mode}` });
  const [a, b] = [{ root: "/r", mode: "a" }, { root: "/r", mode: "b" }];
  assert.equal(await c.get(a), "/r:a:1");
  assert.equal(await c.get(a), "/r:a:1");
  assert.equal(await c.get(b), "/r:b:2");
  c.delete(a);
  assert.equal(await c.get(a), "/r:a:3");
  c.clear();
  assert.equal(c.peek(b), undefined);
});

test("serial runs jobs one at a time in order, and a failed job doesn't stop the next", async () => {
  const run = serial();
  const log = [];
  const job = (name, ms, fail) => async () => {
    log.push(`start ${name}`);
    await sleep(ms);
    log.push(`end ${name}`);
    if (fail) throw new Error(name);
    return name;
  };
  const done = await Promise.allSettled([run(job("a", 20)), run(job("b", 0, true)), run(job("c", 0))]);
  assert.deepEqual(log, ["start a", "end a", "start b", "end b", "start c", "end c"]);
  assert.deepEqual(done.map((r) => r.value ?? r.reason.message), ["a", "b", "c"]);
});

test("singleFlight runs once at a time, and reruns exactly once for the calls that came meanwhile", async () => {
  const releases = [];
  const run = singleFlight(() => new Promise((resolve) => releases.push(resolve)));
  const first = run();
  assert.equal(run(), first);
  run();
  run();
  assert.equal(releases.length, 1);
  releases[0]();
  await next();
  assert.equal(releases.length, 2);
  releases[1]();
  await first;
  assert.equal(releases.length, 2);
  const idle = run();
  assert.equal(releases.length, 3, "an idle call runs at once");
  releases[2]();
  await idle;
});

test("mapLimit keeps the order and the limit, and starts nothing new after a failure", async () => {
  let active = 0;
  let peak = 0;
  const out = await mapLimit([30, 10, 20, 0], 2, async (ms, i) => {
    peak = Math.max(peak, ++active);
    await sleep(ms);
    active--;
    return i;
  });
  assert.deepEqual(out, [0, 1, 2, 3]);
  assert.equal(peak, 2);

  const started = [];
  await assert.rejects(
    mapLimit([1, 2, 3, 4, 5], 2, async (n) => {
      started.push(n);
      if (n === 1) throw new Error("one");
      await sleep(10);
    }),
    /one/,
  );
  await sleep(30);
  assert.deepEqual(started, [1, 2]);
});

test("latestOnly says a load is stale once a later one has started", () => {
  const start = latestOnly();
  const [first, second] = [start(), start()];
  assert.deepEqual([first(), second()], [true, false]);
});

test("fitJson keeps the longest start of the rows whose JSON fits, wrapped or not", () => {
  const rows = ["aaaaa", "bbbbb", "ccccc", "ddddd"];
  // The JSON of n of them is 8n + 1 characters long.
  assert.deepEqual(fitJson(rows, 100), rows);
  assert.deepEqual(fitJson(rows, 25), rows.slice(0, 3));
  assert.deepEqual(fitJson(rows, 24), rows.slice(0, 2));
  // `{"rows":` and `}` add 9.
  assert.deepEqual(fitJson(rows, 26, (all) => ({ rows: all })), rows.slice(0, 2));
  assert.deepEqual(fitJson(rows, 5), []);
  assert.deepEqual(fitJson([], 5), []);
});

test("openDiffWhile opens the viewer before loading, then updates it with the result or the error message", async (t) => {
  const calls = [];
  globalThis.wings = {
    openDiff: async (diff) => (calls.push(["open", diff]), { id: "v1" }),
    updateDiff: async (id, update) => void calls.push(["update", id, update]),
  };
  t.after(() => delete globalThis.wings);
  await openDiffWhile({ title: "T" }, async (id) => (calls.push(["load", id]), { patch: "p" }));
  await openDiffWhile({ title: "U" }, async () => Promise.reject(new Error("git diff failed")));
  assert.deepEqual(calls, [
    ["open", { title: "T" }],
    ["load", "v1"],
    ["update", "v1", { patch: "p" }],
    ["open", { title: "U" }],
    ["update", "v1", { error: "git diff failed" }],
  ]);
});
