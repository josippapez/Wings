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

test("a request or an answer over Wings' 64 KB limit rejects the ask instead of hanging", async () => {
  const { main, a } = plugin();
  main.answer("big", () => "x".repeat(70_000));
  await assert.rejects(a.ask("big"), { message: "The answer to big couldn't be sent: A broadcast is limited to 64 KB" });
  await assert.rejects(a.ask("big", "y".repeat(70_000)), { message: "A broadcast is limited to 64 KB" });
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
  await frames[1].wings.broadcast({ kind: "ask", name: "double", id: "x", request: 1 });
  await settle();
  assert.equal(frames[0].sent.length, 0);
});

test("share sends one update for a burst and none for the same state, and a page opening late gets the current state", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { main, a, b } = plugin();
    const hud = { n: 0 };
    const shared = main.share("hud", () => ({ ...hud }));
    const seen = [];
    a.watch("hud", (state) => seen.push(state));
    await settle();
    assert.deepEqual(seen, [{ n: 0 }]);
    for (const n of [1, 2, 3]) {
      hud.n = n;
      shared.changed();
    }
    mock.timers.tick(299);
    await settle();
    assert.equal(seen.length, 1);
    mock.timers.tick(1);
    await settle();
    assert.deepEqual(seen, [{ n: 0 }, { n: 3 }]);
    shared.changed();
    mock.timers.tick(300);
    await settle();
    assert.equal(seen.length, 2);
    const late = [];
    b.watch("hud", (state) => late.push(state));
    await settle();
    assert.deepEqual(late, [{ n: 3 }]);
    assert.equal(seen.length, 2);
  } finally {
    mock.timers.reset();
  }
});

test("two shared names don't mix", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const { main, a } = plugin();
    const now = { hud: "idle", limits: "5 hour" };
    const hud = main.share("hud", () => now.hud);
    const limits = main.share("limits", () => now.limits);
    const heard = { hud: [], limits: [] };
    a.watch("hud", (state) => heard.hud.push(state));
    a.watch("limits", (state) => heard.limits.push(state));
    await settle();
    now.limits = "weekly";
    limits.changed();
    mock.timers.tick(300);
    await settle();
    assert.deepEqual(heard, { hud: ["idle"], limits: ["5 hour", "weekly"] });
    now.hud = "busy";
    hud.changed();
    mock.timers.tick(300);
    await settle();
    assert.deepEqual(heard, { hud: ["idle", "busy"], limits: ["5 hour", "weekly"] });
  } finally {
    mock.timers.reset();
  }
});
