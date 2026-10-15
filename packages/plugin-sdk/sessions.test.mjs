import assert from "node:assert/strict";
import { test } from "node:test";

import { followSessions } from "./sessions.js";

const settle = () => new Promise((resolve) => setImmediate(resolve));
const entry = (uuid, extra) => ({ type: "assistant", uuid, ...extra });

/** A fake `wings` and a plugin that logs each handler call. `backlog` answers with entries. */
function setup(backlog = async () => []) {
  const on = { panes: [], transcript: [] };
  const subscribe = (list) => (fn) => (list.push(fn), () => list.splice(list.indexOf(fn), 1));
  const log = [];
  const follow = followSessions(
    {
      create: (id) => ({ id }),
      startRun: (s, run) => log.push(`start ${run.startedAt}`),
      backlog: (s, run) => (log.push(`backlog ${run.startedAt}`), backlog()),
      entry: (s, e, { live, thisRun }) => {
        if (e.uuid === "bad") throw new Error("bad entry");
        log.push(`${live ? "live" : "old"} ${e.uuid}${thisRun ? "" : " earlier"}`);
      },
      gone: (s, paneId, open) => log.push(`gone ${paneId} ${open ? "open" : "closed"}`),
    },
    { onPanes: subscribe(on.panes), onTranscript: subscribe(on.transcript) },
  );
  /** Each pane is `[paneId, sessionId, startedAt]`, or `[paneId]` when no session runs in it. */
  const panes = async (...list) => {
    on.panes.forEach((fn) => fn(list.map(([paneId, sessionId, startedAt]) => ({ paneId, session: sessionId ? { sessionId, name: null, state: "idle", startedAt } : null }))));
    await settle();
  };
  const write = (sessionId, e) => on.transcript.forEach((fn) => fn({ sessionId, paneId: "p1", entry: e }));
  return { follow, log, panes, write };
}

test("a new session loads its backlog, then follows what Claude writes", async () => {
  const f = setup(async () => [entry("a"), entry("b")]);
  await f.panes(["p1", "s1", 1000]);
  f.write("s1", entry("c"));
  assert.deepEqual(f.log, ["start 1000", "backlog 1000", "old a", "old b", "live c"]);
  assert.deepEqual(f.follow.sessions(), [{ id: "s1" }]);
});

test("entries written while the backlog loads wait for it, and one that is in both is applied once", async () => {
  let load;
  const f = setup(() => new Promise((resolve) => (load = resolve)));
  await f.panes(["p1", "s1", 1000]);
  f.write("s1", entry("c"));
  f.write("s1", entry("b"));
  load([entry("a"), entry("b"), entry()]);
  await settle();
  f.write("s1", entry("b"));
  f.write("s1", entry());
  f.write("s1", entry("d"));
  assert.deepEqual(f.log.slice(2), ["old a", "old b", "old undefined", "live c", "live undefined", "live d"]);
});

test("a resumed session, the same id with a new startedAt, starts a new run, and a run waits for the one before it", async () => {
  const loads = [];
  const f = setup(() => new Promise((resolve) => loads.push(resolve)));
  await f.panes(["p1", "s1", 1000]);
  loads[0]([]);
  await f.panes(["p1", "s1", 1000]);
  await f.panes(["p1", "s1", 2000]);
  await f.panes(["p1", "s1", 3000]);
  assert.deepEqual(f.log, ["start 1000", "backlog 1000", "start 2000", "backlog 2000"]);
  loads[1]([entry("x")]);
  await settle();
  loads[2]([entry("a")]);
  await settle();
  assert.deepEqual(f.log.slice(4), ["start 3000", "backlog 3000", "old a"]);
});

test("a closed pane calls gone and drops the session, and a pane that stays open says so", async () => {
  const f = setup();
  await f.panes(["p1", "s1", 1000], ["p2", "s2", 1000]);
  await f.panes(["p1"]);
  f.write("s1", entry("late"));
  assert.deepEqual(f.log.slice(4), ["gone p1 open", "gone p2 closed"]);
  assert.deepEqual(f.follow.sessions(), []);
});

test("sidechain entries are skipped, and an entry from before the run started is not this run's", async () => {
  const at = (s) => ({ timestamp: new Date(s * 1000).toISOString() });
  const f = setup(async () => [entry("a", at(7.999)), entry("b", at(8))]);
  await f.panes(["p1", "s1", 10_000]);
  f.write("s1", entry("c", { isSidechain: true }));
  assert.deepEqual(f.log.slice(2), ["old a earlier", "old b"]);
});

test("a failing backlog or entry is logged, and the session still follows what comes next", async (t) => {
  const errors = t.mock.method(console, "error", () => {});
  const f = setup(() => Promise.reject(new Error("no transcript")));
  await f.panes(["p1", "s1", 1000]);
  f.write("s1", entry("bad"));
  f.write("s1", entry("a"));
  assert.deepEqual(f.log, ["start 1000", "backlog 1000", "live a"]);
  assert.equal(errors.mock.callCount(), 2);
});
