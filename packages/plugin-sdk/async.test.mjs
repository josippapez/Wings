import assert from "node:assert/strict";
import { test } from "node:test";
import { setImmediate as next, setTimeout as sleep } from "node:timers/promises";

import { batched, cached, errorMessage, latestOnly, mapLimit, openDiffWhile, serial, singleFlight } from "./async.js";

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

test("batched sends the gets of one microtask turn as one call, and splits it at max", async () => {
  const calls = [];
  const b = batched(async (ids) => (calls.push(ids), ids.map((id) => id * 2)), { max: 3 });
  // The callbacks await different numbers of times before they ask, like projects whose remote is read first.
  const asked = [1, 2, 3, 4, 5, 6, 7].map(async (id) => {
    for (let i = 0; i < id % 3; i++) await null;
    return b.get(id);
  });
  assert.deepEqual(await Promise.all(asked), [2, 4, 6, 8, 10, 12, 14]);
  assert.equal(calls.length, 3, "7 args at max 3 are 3 calls");
  assert.deepEqual(calls.flat().sort(), [1, 2, 3, 4, 5, 6, 7]);
  await b.get(9);
  assert.equal(calls.length, 4, "a get in a later turn is a call of its own");
});

test("batched gives equal keys one slot, and works under cached for a ttl", async (t) => {
  t.mock.timers.enable({ apis: ["Date"] });
  const calls = [];
  const b = batched(async (list) => (calls.push(list), list.map((r) => r.id)), { key: (r) => String(r.id) });
  const c = cached(b.get, { ttlMs: 1000, key: (r) => String(r.id) });
  assert.deepEqual(await Promise.all([c.get({ id: 1 }), c.get({ id: 2 }), c.get({ id: 1 })]), [1, 2, 1]);
  assert.equal(await c.get({ id: 1 }), 1);
  assert.deepEqual(calls.map((l) => l.map((r) => r.id)), [[1, 2]], "the duplicate went out once and the kept value not at all");
  const dup = batched(async (ids) => (calls.push(ids), ids), {});
  await Promise.all([dup.get("a"), dup.get("a")]);
  assert.deepEqual(calls.at(-1), ["a"]);
});

test("batched rejects every get of a failed call, an Error value only its own, and a miscounted answer all", async () => {
  const boom = new Error("boom");
  const failing = batched(async () => {
    throw boom;
  });
  assert.deepEqual(await Promise.allSettled([failing.get(1), failing.get(2)]), [{ status: "rejected", reason: boom }, { status: "rejected", reason: boom }]);
  const own = batched(async (ids) => ids.map((id) => (id === 2 ? new Error("no 2") : id)));
  const [one, two] = await Promise.allSettled([own.get(1), own.get(2)]);
  assert.deepEqual([one.value, two.reason.message], [1, "no 2"]);
  const short = batched(async () => []);
  await assert.rejects(short.get(1), /gave 0 values for 1 args/);
  const chunks = batched(async (ids) => { if (ids[0] === 1) throw boom; return ids; }, { max: 1 });
  const out = await Promise.allSettled([chunks.get(1), chunks.get(2)]);
  assert.deepEqual(out.map((o) => o.status), ["rejected", "fulfilled"], "a chunk fails alone");
});
