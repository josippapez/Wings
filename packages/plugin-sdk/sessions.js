// Follows the Claude sessions that run in panes: which pane runs which session, each run's backlog from the
// transcript, then the entries Claude appends, oldest first and each once.

// The process start time is only to the second, so entries a little before it still belong to the run.
const SLACK = 2000;

/**
 * Tracks the sessions `wings.onPanes` shows and calls your handlers, one run of one session at a time. A new `claude`
 * process (a new session, or a resumed one with a new `startedAt`) starts a run: `startRun`, then `backlog`, then
 * `entry` for each entry it returned. Entries Claude writes while the backlog loads wait for it, and an entry that is
 * in both is passed on once. Sidechain entries are skipped. A session no pane runs any more is dropped after `gone`.
 * A `startRun`, `backlog` or `entry` that throws is logged and the session carries on.
 */
export function followSessions({ create, pane, startRun, backlog, entry, gone, changed }, w = globalThis.wings) {
  /** By session id. `queue` holds live entries while the run's backlog loads, then it is null. */
  const tracked = new Map();
  /** Pane id to the id of the session it runs. */
  let panes = new Map();

  function deliver(t, e, live) {
    // The backlog is read after Wings starts following the transcript, so it can hold what `onTranscript` sends too.
    if (e.isSidechain || (live && t.seen.has(e.uuid))) return;
    const at = Date.parse(e.timestamp ?? "") || Date.now();
    try {
      entry(t.value, e, { live, thisRun: at >= t.run.startedAt - SLACK });
    } catch (error) {
      console.error(error);
    }
  }

  async function load(t, run) {
    // A newer run, or a session no pane runs any more, makes this one pointless.
    const stale = () => tracked.get(run.sessionId) !== t || t.run !== run;
    if (stale()) return;
    let entries = [];
    try {
      startRun(t.value, run);
      entries = await backlog(t.value, run);
    } catch (error) {
      console.error(error);
    }
    if (stale()) return;
    t.seen = new Set(entries.map((e) => e.uuid).filter(Boolean));
    const queued = t.queue;
    t.queue = null;
    for (const e of entries) deliver(t, e, false);
    for (const e of queued) deliver(t, e, true);
    changed?.(t.value);
  }

  const offPanes = w.onPanes((list) => {
    const open = new Set(list.map((p) => p.paneId));
    const next = new Map(list.filter((p) => p.session).map((p) => [p.paneId, p]));
    for (const [paneId, id] of panes) {
      if (next.get(paneId)?.session.sessionId === id) continue;
      const t = tracked.get(id);
      gone?.(t.value, paneId, open.has(paneId));
      changed?.(t.value);
    }
    for (const [paneId, p] of next) {
      const run = p.session;
      let t = tracked.get(run.sessionId);
      if (!t) tracked.set(run.sessionId, (t = { value: create(run.sessionId), run: null, queue: [], seen: new Set(), turn: Promise.resolve() }));
      pane?.(t.value, p);
      if (t.run?.startedAt !== run.startedAt) {
        // A handler can change the session after an await, so a run waits for the one before it.
        t.run = run;
        t.queue = [];
        t.turn = t.turn.then(() => load(t, run));
      }
      changed?.(t.value);
    }
    panes = new Map([...next].map(([paneId, p]) => [paneId, p.session.sessionId]));
    const running = new Set(panes.values());
    for (const id of tracked.keys()) if (!running.has(id)) tracked.delete(id);
  });

  const offTranscript = w.onTranscript(({ sessionId, entry: e }) => {
    const t = tracked.get(sessionId);
    if (!t) return;
    if (t.queue) return void t.queue.push(e);
    deliver(t, e, true);
    changed?.(t.value);
  });

  return {
    sessions: () => [...tracked.values()].map((t) => t.value),
    get: (sessionId) => tracked.get(sessionId)?.value,
    loaded: (sessionId) => tracked.get(sessionId)?.queue === null,
    stop() {
      offPanes();
      offTranscript();
      tracked.clear();
    },
  };
}
