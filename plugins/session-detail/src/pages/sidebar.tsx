// The Session sidebar page: what one Claude session is doing now, the files it edited and its recent tool calls.
// The plugin's main script keeps the state and shares it here.

import { useState } from "react";
import { useShared } from "wings-plugin-sdk/react";
import { Empty, Loading, Select } from "wings-plugin-ui";

import { Activity } from "../ui/Activity";
import { Files } from "../ui/Files";
import { Now } from "../ui/Now";
import { Stats } from "../ui/Stats";
import { Todos } from "../ui/Todos";
import { ch } from "../lib/messages";
import "./sidebar.css";

export const page = { place: "sidebar" };

/** Rows are by session and key, since a key only names a row within its session. */
const rowId = (sessionId: string, key: string) => `${sessionId}\n${key}`;

export default function Sidebar() {
  const sessions = useShared(ch, "sessions");
  /** The session you picked; otherwise the one that changed last. */
  const [picked, setPicked] = useState<string | null>(null);
  /** Activity rows you opened, by row id. */
  const [opened, setOpened] = useState(() => new Set<string>());
  /** Show only your messages, highlighted commands and failures. */
  const [highlightsOnly, setHighlightsOnly] = useState(false);

  function toggle(sessionId: string, key: string) {
    const id = rowId(sessionId, key);
    setOpened((open) => {
      const next = new Set(open);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }

  if (sessions === null) return <Loading />;
  if (sessions.length === 0) return <Empty title="No Claude session in your panes." description="Run claude in a pane and its activity shows here." />;
  const s = sessions.find((x) => x.id === picked) ?? sessions[0];
  return (
    <>
      {sessions.length > 1 && <Select label="Session" value={s.id} onChange={setPicked} options={sessions.map((x) => ({ value: x.id, label: `${x.project} · ${x.name ?? x.id.slice(0, 8)}` }))} />}
      <Stats s={s} />
      {s.state === "working" && s.running.length ? <Now running={s.running} /> : null}
      {s.todos.length ? <Todos todos={s.todos} /> : null}
      {s.files.length ? <Files key={s.id} sessionId={s.id} files={s.files} /> : null}
      <Activity sessionId={s.id} items={s.items} highlightsOnly={highlightsOnly} setHighlightsOnly={setHighlightsOnly} isOpen={(key) => opened.has(rowId(s.id, key))} toggle={(key) => toggle(s.id, key)} />
    </>
  );
}
