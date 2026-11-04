import assert from "node:assert/strict";
import { test } from "node:test";
import type { Agent } from "./api.ts";
import { syncPaneSession, type PaneSession } from "./pane-session.ts";
import { noResume } from "./sleep.ts";

const id = "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d";
const other = "1aa900ec-ed63-4df5-97c1-eecebeb8ef8d";
const agent = (over: Partial<Agent> = {}): Agent => ({
  paneId: "p", spaceId: "s", pid: 10, kind: "claude", sessionId: id, name: "Morning review", state: "idle", waitingFor: null,
  args: ["--model", "opus"], startedAt: 1000, stateSince: null, direct: true, summary: null, ...over,
});
const meta = (over: Partial<PaneSession> = {}): PaneSession => ({
  paneId: "p", sessionId: null, kind: "claude", name: null, live: false, args: [], openedBy: "schedules", ...noResume, ...over,
});

test("another session in the scheduled pane loses its origin and old title, even in the same process", () => {
  const scheduled = syncPaneSession(meta(), agent());
  const manual = syncPaneSession(scheduled, agent({ sessionId: other, name: null, args: [] }));
  assert.equal(manual.openedBy, undefined);
  assert.equal(manual.name, null);
  assert.equal(manual.sessionId, other);
  assert.deepEqual(manual.args, []);
});

test("returning to the shell clears the origin and all remembered session details", () => {
  const shell = syncPaneSession(syncPaneSession(meta(), agent()), undefined);
  assert.equal(shell.openedBy, undefined);
  assert.equal(shell.name, null);
  assert.equal(shell.sessionId, null);
  assert.deepEqual(shell.args, []);
  assert.equal(shell.live, false);
});

test("switching to an agent without resume support cannot keep the old session or flags", () => {
  for (const kind of ["codex", "cursor", "grok"] as const) {
    const switched = syncPaneSession(syncPaneSession(meta(), agent()), agent({ kind, pid: 20, sessionId: null, name: null, args: [] }));
    assert.equal(switched.openedBy, undefined, kind);
    assert.equal(switched.sessionId, null, kind);
    assert.equal(switched.name, null, kind);
    assert.deepEqual(switched.args, [], kind);
    assert.equal(switched.live, true, kind);
  }
});

test("agents without session ids bind to their process and lose the origin on process replacement", () => {
  for (const kind of ["codex", "cursor", "grok"] as const) {
    const running = agent({ kind, sessionId: null, name: null, args: [] });
    const scheduled = syncPaneSession(meta(), running);
    assert.equal(scheduled.live, true, kind);
    assert.equal(scheduled.openedBy, "schedules", kind);
    assert.equal(syncPaneSession(scheduled, running), scheduled, "unchanged detection does not render again");
    assert.equal(syncPaneSession(scheduled, { ...running, startedAt: 2000 }).openedBy, undefined, "a reused pid is still another process");
    assert.equal(syncPaneSession(scheduled, undefined).openedBy, undefined, "the agent exits");
  }
});

test("sleep and waking the same session preserve its plugin origin and title", () => {
  const scheduled = syncPaneSession(meta(), agent());
  const sleeping = { ...scheduled, asleep: true };
  assert.equal(syncPaneSession(sleeping, undefined), sleeping);
  const waking = syncPaneSession({ ...sleeping, asleep: false, resumeAt: 2000 }, agent({ pid: 20, startedAt: 2000, name: null }));
  assert.equal(waking.openedBy, "schedules");
  assert.equal(waking.name, "Morning review");
  assert.equal(waking.asleep, false);
  assert.equal(waking.resumeAt, null);
});

test("restored origin is checked against the saved session and process before another session can inherit it", () => {
  const scheduled = syncPaneSession(meta(), agent());
  const restored = { ...scheduled, live: false, name: null };
  assert.equal(syncPaneSession(restored, agent()).openedBy, "schedules");
  assert.equal(syncPaneSession(restored, agent({ sessionId: other })).openedBy, undefined);
  const codex = syncPaneSession(meta(), agent({ kind: "codex", sessionId: null, name: null }));
  assert.equal(syncPaneSession({ ...codex, live: false }, agent({ kind: "codex", sessionId: null, pid: 20 })).openedBy, undefined);
});

test("a session id arriving after process detection keeps the origin; later session changes clear it", () => {
  const starting = syncPaneSession(meta(), agent({ sessionId: null, name: null }));
  const detected = syncPaneSession(starting, agent());
  assert.equal(detected.openedBy, "schedules");
  assert.equal(syncPaneSession(detected, agent({ sessionId: null, name: null })).sessionId, id, "a temporarily unavailable session file keeps the known session");
  assert.equal(syncPaneSession(detected, agent({ sessionId: other })).openedBy, undefined);
});

test("switching between resumable agents replaces the title and flags", () => {
  const switched = syncPaneSession(syncPaneSession(meta(), agent()), agent({ kind: "opencode", sessionId: "ses_0123456789", name: null, args: [] }));
  assert.equal(switched.kind, "opencode");
  assert.equal(switched.openedBy, undefined);
  assert.equal(switched.name, null);
  assert.deepEqual(switched.args, []);
});

test("a title update in the current session replaces the saved title without losing its origin", () => {
  const current = syncPaneSession(meta(), agent());
  const renamed = syncPaneSession(current, agent({ name: "Updated title" }));
  assert.equal(renamed.name, "Updated title");
  assert.equal(renamed.openedBy, "schedules");
});

test("any plugin origin is scoped to its agent, and an unlaunched pane keeps its origin", () => {
  const unopened = meta({ paneId: null, openedBy: "worktrees" });
  assert.equal(syncPaneSession(unopened, undefined), unopened);
  const running = syncPaneSession(unopened, agent());
  assert.equal(running.openedBy, "worktrees");
  assert.equal(syncPaneSession(running, agent({ sessionId: other })).openedBy, undefined);
});

test("resume and restore wait for the session id before deciding whether a new process belongs to the plugin", () => {
  const scheduled = syncPaneSession(meta(), agent());
  for (const waiting of [{ ...scheduled, resumeAt: 2000 }, { ...scheduled, live: false }]) {
    const starting = syncPaneSession(waiting, agent({ pid: 20, startedAt: 2000, sessionId: null, name: null }));
    assert.equal(starting.openedBy, "schedules");
    assert.equal(starting.sessionId, id);
    assert.equal(starting.name, "Morning review");
    assert.equal(syncPaneSession(starting, agent({ pid: 20, startedAt: 2000 })).openedBy, "schedules");
    assert.equal(syncPaneSession(starting, agent({ pid: 20, startedAt: 2000, sessionId: other })).openedBy, undefined);
  }
});

test("an unrelated pane binding change cannot cancel sleep while the old idle process exits", () => {
  const scheduled = syncPaneSession(meta(), agent());
  const sleeping = { ...scheduled, asleep: true };
  assert.equal(syncPaneSession(sleeping, agent()), sleeping);
  assert.equal(syncPaneSession(sleeping, undefined), sleeping);
});
