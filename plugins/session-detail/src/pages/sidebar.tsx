// The Session sidebar page: what one Claude session is doing now, the files it edited and its recent tool calls.
// The plugin's main script keeps the state and broadcasts it here.

import { useEffect, useRef, useState } from "react";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Empty, Loading, Select, useMainState } from "wings-plugin-ui";

import { Activity } from "../ui/Activity";
import { Files } from "../ui/Files";
import { Now } from "../ui/Now";
import { Stats } from "../ui/Stats";
import { Todos } from "../ui/Todos";
import type { Detail, ToMain, ToPage } from "../lib/types";
import "./sidebar.css";

export const page = { place: "sidebar" };

const send = (message: ToMain) => void wings.broadcast(message);
/** Rows are by session and key, since a key only names a row within its session. */
const rowId = (sessionId: string, key: string) => `${sessionId}\n${key}`;

export default function Sidebar() {
  const sessions = useMainState<Extract<ToPage, { type: "sessions" }>>("sessions")?.sessions ?? null;
  /** The session you picked; otherwise the one that changed last. */
  const [picked, setPicked] = useState<string | null>(null);
  /** Activity rows you opened, by row id, and the detail the main script sent for each. */
  const [opened, setOpened] = useState(() => new Set<string>());
  /** Show only your messages, highlighted commands and failures. */
  const [highlightsOnly, setHighlightsOnly] = useState(false);
  const [details, setDetails] = useState(() => new Map<string, Detail>());
  /** Rows whose detail was asked for again and hasn't come yet. */
  const asked = useRef(new Set<string>());

  useBroadcast((message) => {
    const m = message as ToPage | null;
    if (m?.type !== "detail") return;
    const id = rowId(m.sessionId, m.key);
    asked.current.delete(id);
    setDetails((d) => new Map(d).set(id, m.detail));
  });

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

  if (sessions === null) return <Loading />;
  if (sessions.length === 0) return <Empty title="No Claude session in your panes." description="Run claude in a pane and its activity shows here." />;
  const s = sessions.find((x) => x.id === picked) ?? sessions[0];
  return (
    <>
      {sessions.length > 1 && <Select label="Session" value={s.id} onChange={setPicked} options={sessions.map((x) => ({ value: x.id, label: `${x.project} · ${x.name ?? x.id.slice(0, 8)}` }))} />}
      <Stats s={s} />
      {s.state === "working" && s.running.length ? <Now running={s.running} /> : null}
      {s.todos.length ? <Todos todos={s.todos} /> : null}
      {s.files.length ? <Files files={s.files} openDiff={(path) => openDiff(s.id, path)} /> : null}
      <Activity items={s.items} highlightsOnly={highlightsOnly} setHighlightsOnly={setHighlightsOnly} isOpen={(key) => opened.has(rowId(s.id, key))} detailOf={(key) => details.get(rowId(s.id, key))} toggle={(key) => toggle(s.id, key)} />
    </>
  );
}
