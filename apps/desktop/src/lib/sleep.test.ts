/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { Agent } from "./api.ts";
import { PaneInput } from "./pane-input.ts";
import { isSessionIdOf, resumeLine } from "./resume.ts";
import { canSleep, nextResumeDeadline, noResume, phaseOf, planSleep, RESUME_TIMEOUT_MS, resumeExpired, settled, type ResumeMeta } from "./sleep.ts";

const id = "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d";
const agent = (over: Partial<Agent> = {}): Agent => ({
  paneId: "p", spaceId: "s", pid: 1, kind: "claude", sessionId: id, name: null, state: "idle", waitingFor: null, args: [], startedAt: 0, stateSince: null, direct: true, summary: null, ...over,
});
const check = (over: Partial<Parameters<typeof canSleep>[0]> = {}) => canSleep({ agent: agent(), visible: false, draft: false, busy: false, background: false, ...over });

test("an idle agent out of sight, with a session, may sleep", () => {
  assert.ok(check());
  assert.ok(check({ agent: agent({ kind: "opencode", sessionId: "ses_0123456789abcdef" }) }));
});

test("working, blocked and unseen-done agents never sleep", () => {
  for (const state of ["working", "blocked", "done"] as const) assert.ok(!check({ agent: agent({ state }) }), state);
});

test("an agent on screen, with a draft typed, already asleep, or without a usable session stays awake", () => {
  assert.ok(!check({ visible: true }));
  assert.ok(!check({ draft: true }));
  assert.ok(!check({ busy: true }));
  assert.ok(!check({ agent: undefined }));
  assert.ok(!check({ agent: agent({ sessionId: null }) }));
  assert.ok(!check({ agent: agent({ sessionId: "not a session; rm -rf ~" }) }));
  assert.ok(!check({ agent: agent({ kind: "opencode", sessionId: id }) }), "a Claude id is not an opencode session");
});

test("an agent with background work stays awake", () => {
  assert.ok(!check({ background: true }));
});

test("an agent started through a wrapper stays awake", () => {
  assert.ok(!check({ agent: agent({ direct: false }) }));
});

test("the clock starts when a pane becomes eligible and runs out after the limit", () => {
  const min = 60_000;
  let plan = planSleep(new Map(), ["a"], 0, 15);
  assert.deepEqual(plan.due, []);
  assert.equal(plan.nextAt, 15 * min);
  plan = planSleep(plan.since, ["a"], 14 * min, 15);
  assert.deepEqual(plan.due, []);
  plan = planSleep(plan.since, ["a"], 15 * min, 15);
  assert.deepEqual(plan.due, ["a"]);
  assert.equal(plan.nextAt, null);
});

test("a pane that stops being eligible starts over", () => {
  const min = 60_000;
  let plan = planSleep(new Map(), ["a", "b"], 0, 15);
  plan = planSleep(plan.since, ["b"], 10 * min, 15);
  assert.ok(!plan.since.has("a"));
  plan = planSleep(plan.since, ["a", "b"], 12 * min, 15);
  assert.equal(plan.since.get("a"), 12 * min);
  assert.deepEqual(planSleep(plan.since, ["a", "b"], 15 * min, 15).due, ["b"]);
  assert.equal(plan.nextAt, 15 * min, "the next timer is the earliest clock");
});

test("typed text counts as a draft until it is submitted or cleared", () => {
  const input = new PaneInput();
  assert.ok(!input.hasDraft("p"));
  input.note("p", "fix");
  assert.ok(input.hasDraft("p"));
  input.note("p", "\r");
  assert.ok(!input.hasDraft("p"));
  input.note("p", "x");
  input.note("p", "\x03");
  assert.ok(!input.hasDraft("p"));
});

test("focus reports, mouse reports and a lone Escape are not typing", () => {
  const input = new PaneInput();
  for (const data of ["\x1b[I", "\x1b[O", "\x1b[<64;10;5M", "\x1b[<0;3;4m", "\x1b[M #!", "\x1b"]) input.note("p", data);
  assert.ok(!input.hasDraft("p"));
  input.note("p", "\x1b[A");
  assert.ok(input.hasDraft("p"), "history recall fills the prompt");
});

test("the terminal's replies to a program starting up are not typing", () => {
  const input = new PaneInput();
  for (const data of ["\x1b[?6c", "\x1b[>0;276;0c", "\x1b[0n", "\x1b[?2026;2$y", "\x1b]11;rgb:1414/1212/1717\x1b\\", "\x1b]10;rgb:ffff/ffff/ffff\x07", "\x1bP>|xterm.js(5.5.0)\x1b\\", "\x1b[?6c\x1b[0n"])
    input.note("p", data);
  assert.ok(!input.hasDraft("p"));
});

test("a message left unsent keeps the agent awake but is no draft of yours, and Enter or Ctrl-C clears it", () => {
  const input = new PaneInput();
  input.noteMessage("p");
  assert.ok(input.hasDraft("p") && !input.hasTyped("p"));
  input.note("p", "\x03");
  assert.ok(!input.hasDraft("p"));
  input.noteMessage("p");
  input.note("p", "x");
  input.sent("p");
  assert.ok(!input.hasDraft("p") && !input.hasTyped("p"), "sending sends your typing with it");
});

test("a paste that ends in Enter is still a draft", () => {
  const input = new PaneInput();
  input.note("p", "\x1b[200~one\ntwo\r");
  assert.ok(input.hasDraft("p"));
});

const meta = (over: Partial<ResumeMeta> = {}): ResumeMeta => ({ paneId: null, sessionId: id, ...noResume, ...over });

test("a restored pane not opened yet, then asleep, show their own rows", () => {
  assert.equal(phaseOf(meta(), undefined), "restored");
  assert.equal(phaseOf(meta({ paneId: "p", asleep: true }), undefined), "sleeping");
  assert.equal(phaseOf(meta({ paneId: "p" }), undefined), null, "a started pane with no resume pending is a plain shell");
  assert.equal(phaseOf(meta({ sessionId: null }), undefined), null);
});

test("resume: the row stays as 'starting' from the request until the agent is seen, then hands over", () => {
  // Click on the restored row: the pane starts (it gets a pane id) before any agent is detected. The row never vanishes.
  const asked = meta({ resumeAt: 1000 });
  assert.equal(phaseOf(asked, undefined), "starting");
  const started = { ...asked, paneId: "p" };
  assert.equal(phaseOf(started, undefined), "starting", "pane started, agent not detected yet");
  assert.ok(!settled(started, undefined));
  // Detection reports the agent: no pending row, and the resume state is cleared.
  assert.equal(phaseOf(started, agent()), null);
  assert.ok(settled(started, agent()));
});

test("waking a sleeping agent goes through the same starting phase", () => {
  const sleeping = meta({ paneId: "p", asleep: true });
  assert.equal(phaseOf(sleeping, undefined), "sleeping");
  const waking = { ...sleeping, asleep: false, resumeAt: 5000 };
  assert.equal(phaseOf(waking, undefined), "starting");
  assert.ok(settled(waking, agent()));
});

test("an agent still running after the exit was sent has its live row, not a sleeping one", () => {
  assert.equal(phaseOf(meta({ paneId: "p", asleep: true }), agent()), null);
});

test("a resume that never shows an agent becomes 'failed' after the timeout and stays a row", () => {
  const m = meta({ paneId: "p", resumeAt: 1000 });
  assert.equal(nextResumeDeadline([m]), 1000 + RESUME_TIMEOUT_MS);
  assert.ok(!resumeExpired(1000, 1000 + RESUME_TIMEOUT_MS - 1));
  assert.ok(resumeExpired(1000, 1000 + RESUME_TIMEOUT_MS));
  const failed = { ...m, resumeFailed: true };
  assert.equal(phaseOf(failed, undefined), "failed");
  assert.equal(nextResumeDeadline([failed]), null, "a failed resume waits for a retry, not for another timeout");
  // Retry: a fresh request clears the failure and starts the clock again.
  assert.equal(phaseOf({ ...failed, resumeFailed: false, resumeAt: 30_000 }, undefined), "starting");
});

test("the earliest pending deadline wins", () => {
  assert.equal(nextResumeDeadline([meta(), meta({ resumeAt: 9000 }), meta({ resumeAt: 4000 })]), 4000 + RESUME_TIMEOUT_MS);
  assert.equal(nextResumeDeadline([meta()]), null);
});

test("the resume line per agent kind", () => {
  assert.equal(resumeLine("claude", id, ["--model", "opus"]), `claude --resume ${id} --model opus\r`);
  assert.equal(resumeLine("opencode", "ses_0123456789abcdef"), "opencode --session ses_0123456789abcdef\r");
  assert.ok(isSessionIdOf("opencode", "ses_0123456789abcdef"));
  assert.ok(!isSessionIdOf("opencode", "ses_x; rm -rf ~"));
});
