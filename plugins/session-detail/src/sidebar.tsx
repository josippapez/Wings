// The Session sidebar: what one Claude session is doing now, the files it edited and its recent tool calls.
// The plugin's main script keeps the state and broadcasts it here.

import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

import { Activity } from "./components/Activity";
import { Files } from "./components/Files";
import { Now } from "./components/Now";
import { Stats } from "./components/Stats";
import { Todos } from "./components/Todos";
import type { Detail, SessionView, ToMain, ToPage } from "./lib/types";

const send = (message: ToMain) => void wings.broadcast(message);
/** Rows are by session and key, since a key only names a row within its session. */
const rowId = (sessionId: string, key: string) => `${sessionId}\n${key}`;

function Sidebar() {
  const [sessions, setSessions] = useState<SessionView[] | null>(null);
  /** The session you picked; otherwise the one that changed last. */
  const [picked, setPicked] = useState<string | null>(null);
  /** Activity rows you opened, by row id, and the detail the main script sent for each. */
  const [opened, setOpened] = useState(() => new Set<string>());
  /** Show only your messages, highlighted commands and failures. */
  const [highlightsOnly, setHighlightsOnly] = useState(false);
  const [details, setDetails] = useState(() => new Map<string, Detail>());
  /** Rows whose detail was asked for again and hasn't come yet. */
  const asked = useRef(new Set<string>());

  useEffect(() => {
    const off = wings.onBroadcast((message) => {
      const m = message as ToPage | null;
      if (m?.type === "detail") {
        const id = rowId(m.sessionId, m.key);
        asked.current.delete(id);
        setDetails((d) => new Map(d).set(id, m.detail));
      }
      else if (m?.type === "sessions") setSessions(m.sessions);
    });
    send({ type: "hello" });
    return off;
  }, []);

  // A row opened while its tool call ran asks again once the call is done, so it doesn't stay "still running".
  useEffect(() => {
    for (const s of sessions ?? []) {
      for (const item of s.items) {
        const id = rowId(s.id, item.key);
        const d = details.get(id);
        if (!opened.has(id) || !d || d.done === item.done || asked.current.has(id)) continue;
        asked.current.add(id);
        send({ type: "detail", sessionId: s.id, key: item.key });
      }
    }
  }, [sessions, opened, details]);

  function toggle(sessionId: string, key: string) {
    const id = rowId(sessionId, key);
    const next = new Set(opened);
    if (next.has(id)) next.delete(id);
    else {
      next.add(id);
      if (!details.has(id)) send({ type: "detail", sessionId, key });
    }
    setOpened(next);
  }

  const openDiff = (sessionId: string, path: string | null) => send({ type: "diff", sessionId, path });

  if (sessions === null) return <p className="muted">Loading…</p>;
  if (sessions.length === 0) {
    return (
      <div className="empty">
        <p>No Claude session in your panes.</p>
        <p className="muted">Run claude in a pane and its activity shows here.</p>
      </div>
    );
  }
  const s = sessions.find((x) => x.id === picked) ?? sessions[0];
  return (
    <>
      {sessions.length > 1 && (
        <select aria-label="Session" value={s.id} onChange={(e) => setPicked(e.target.value)}>
          {sessions.map((x) => (
            <option key={x.id} value={x.id}>
              {`${x.project} · ${x.name ?? x.id.slice(0, 8)}`}
            </option>
          ))}
        </select>
      )}
      <Stats s={s} />
      {s.state === "working" && s.running.length ? <Now running={s.running} /> : null}
      {s.todos.length ? <Todos todos={s.todos} /> : null}
      {s.files.length ? <Files files={s.files} openDiff={(path) => openDiff(s.id, path)} /> : null}
      <Activity items={s.items} highlightsOnly={highlightsOnly} setHighlightsOnly={setHighlightsOnly} isOpen={(key) => opened.has(rowId(s.id, key))} detailOf={(key) => details.get(rowId(s.id, key))} toggle={(key) => toggle(s.id, key)} />
    </>
  );
}

createRoot(document.getElementById("app")!).render(<Sidebar />);
