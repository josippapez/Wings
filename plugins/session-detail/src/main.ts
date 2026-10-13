// Follows each Claude session in a pane for a sidebar with its recent activity.
// It reads the newest transcript entries once per run of `claude`, then only what Claude appends. A resumed
// session's transcript also holds earlier runs, which only count for the model and context.

import { openChanges } from "./lib/diff";
import { emptyTokens, ingest, session, sessions, type Entry, type Session } from "./lib/session";
import type { Detail, ToMain, ToPage } from "./lib/types";
import { snapshot } from "./lib/view";

// Many entries are small attachments, so this reaches back a bit further than the number suggests.
const BACKLOG = 500;

/** The sessions panes run now, by pane id. */
let panes = new Map<string, Session>();

const send = (message: ToPage) => wings.broadcast(message);

// Changes come in bursts while Claude works, so the sidebar updates at most every 300 ms.
let timer: ReturnType<typeof setTimeout> | null = null;
function changed() {
  timer ??= setTimeout(flush, 300);
}
function flush() {
  timer = null;
  void send({ type: "sessions", sessions: snapshot() });
}

wings.onPanes(async (list) => {
  const next = new Map(list.filter((p) => p.session).map((p) => [p.paneId, p]));
  for (const [paneId, s] of panes) {
    if (next.get(paneId)?.session?.sessionId !== s.id) s.paneId = null;
  }
  const current = new Map<string, Session>();
  for (const [paneId, pane] of next) {
    const run = pane.session!;
    const s = session(run.sessionId);
    // A new `claude` process, even for the same session after a resume, starts the activity afresh.
    const isNew = !s.loaded || s.startedAt !== run.startedAt;
    Object.assign(s, { paneId, cwd: pane.cwd, project: pane.project, name: run.name, state: run.state });
    current.set(paneId, s);
    if (isNew) {
      Object.assign(s, { loaded: true, startedAt: run.startedAt, items: [], tools: new Map(), files: new Map(), todos: [], tasks: new Map(), creating: new Map(), lastPrompt: null, tokens: emptyTokens(), partial: false });
      try {
        const backlog = (await wings.transcript(s.id, ["assistant", "user", "attachment"], { last: BACKLOG })) as unknown as Entry[];
        // A full backlog that starts after this run began missed some of it, so its token total is a floor.
        s.partial = backlog.length >= BACKLOG && (Date.parse(backlog[0]?.timestamp as string) || 0) > (s.startedAt ?? 0);
        for (const entry of backlog) ingest(s, entry);
      } catch (error) {
        console.error(error);
      }
    }
    changed();
  }
  panes = current;
  if (current.size === 0) void send({ type: "sessions", sessions: [] });
});

wings.onTranscript(({ sessionId, entry }) => {
  const s = sessions.get(sessionId);
  if (!s?.loaded) return;
  ingest(s, entry as Entry);
  changed();
});

// A sidebar that just opened asks for the current state, and one that opens a row asks for its detail.
wings.onBroadcast((data) => {
  const message = data as ToMain | null;
  if (message?.type === "hello") void send({ type: "sessions", sessions: snapshot() });
  if (message?.type === "diff") void openChanges(message.sessionId, message.path ?? null).catch(console.error);
  if (message?.type === "detail") {
    const item = sessions.get(message.sessionId)?.items.find((i) => i.key === message.key);
    if (!item) return;
    const { kind, name, at, ms, error, done, input, result, text } = item;
    const detail: Detail = { kind, name, at, ms, error, done, input, result, text };
    void send({ type: "detail", sessionId: message.sessionId, key: message.key, detail });
  }
});
