// The HUD sidebar: usage limits and memory, then one session's context, MCP servers and skills. The plugin's
// main script keeps the state and broadcasts it here.

import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

import { gb, tokens } from "./lib/format";
import type { Server, Snapshot } from "./lib/types";

/** A labelled bar. `levels` are where it changes colour. */
function Gauge({ label, percent, note, levels }: { label: string; percent: number; note: string | null; levels: [number, number] }) {
  const level = percent >= levels[1] ? " danger" : percent >= levels[0] ? " warn" : "";
  return (
    <div className="gauge">
      <span>{label}</span>
      <span className="value">{percent}%</span>
      <div className={`bar${level}`} role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
        <span style={{ width: `${Math.min(100, percent)}%` }} />
      </div>
      {note ? <span className="note">{note}</span> : null}
    </div>
  );
}

const states = { connected: "Connected", failed: "Failed", connecting: "Connecting", "sign-in": "Needs sign-in" };

function ServerRow({ server }: { server: Server }) {
  const tools = server.tools ? `, ${server.tools} ${server.tools === 1 ? "tool" : "tools"}` : "";
  return (
    <li title={`${server.name}: ${states[server.state]}${tools}`}>
      <span className={`dot ${server.state}`} aria-hidden />
      <span className="name">{server.name}</span>
      {server.state === "failed" ? (
        <span className="tag danger" title={server.error || undefined}>
          {server.error || "Failed"}
        </span>
      ) : server.state === "connecting" ? (
        <span className="tag">Connecting</span>
      ) : null}
      {server.lastFailed ? <span className="tag warn">Last call failed</span> : server.used ? <span className="tag">Used</span> : null}
    </li>
  );
}

function Hud() {
  const [hud, setHud] = useState<Snapshot | null>(null);
  /** The session you picked; otherwise the first. */
  const [picked, setPicked] = useState<string | null>(null);

  useEffect(() => {
    const off = wings.onBroadcast((message) => {
      if ((message as Snapshot | null)?.type === "hud") setHud(message as Snapshot);
    });
    void wings.broadcast({ type: "hello" });
    return off;
  }, []);

  if (hud === null) return <p className="muted">Loading…</p>;
  const { overview: o, sessions } = hud;
  const usage = (
    <>
      <h2>Usage</h2>
      <div className="card">
        {o.fiveHour && <Gauge label="5-hour limit" percent={o.fiveHour.percent} note={o.fiveHour.resets && `Resets in ${o.fiveHour.resets}`} levels={[75, 90]} />}
        {o.sevenDay && <Gauge label="Weekly limit" percent={o.sevenDay.percent} note={o.sevenDay.resets && `Resets in ${o.sevenDay.resets}`} levels={[75, 90]} />}
        {!o.statusline && (
          <p className="hint muted">
            Your 5-hour and weekly limits show after Claude Code's next reply, on Pro and Max plans. If they don't, connect Claude Code in Plugins.
          </p>
        )}
        {o.memory && <Gauge label="Memory" percent={o.memory.percent} note={`${gb(o.memory.used)} of ${gb(o.memory.total)} in use`} levels={[75, 90]} />}
      </div>
    </>
  );

  if (sessions.length === 0) {
    return (
      <>
        {usage}
        <div className="empty">
          <p>No Claude session in your panes.</p>
          <p className="muted">Run claude in a pane to see its context, MCP servers and skills.</p>
        </div>
      </>
    );
  }
  const s = sessions.find((x) => x.id === picked) ?? sessions[0];
  const servers = s.servers.filter((x) => x.state !== "sign-in");
  const signIn = s.servers.filter((x) => x.state === "sign-in");
  return (
    <>
      {usage}
      {sessions.length > 1 && (
        <select aria-label="Session" value={s.id} onChange={(e) => setPicked(e.target.value)}>
          {sessions.map((x) => (
            <option key={x.id} value={x.id}>
              {`${x.project} · ${x.name ?? x.id.slice(0, 8)}`}
            </option>
          ))}
        </select>
      )}
      <h2>Context</h2>
      <div className="card">
        {s.context ? (
          <Gauge
            label="Context window"
            percent={s.context.percent}
            note={[`${tokens(s.context.tokens)} of ${tokens(s.context.window)} tokens`, s.context.cache && `prompt cache ${s.context.cache.toLowerCase()}`].filter(Boolean).join(" · ")}
            levels={[70, 85]}
          />
        ) : (
          <p className="muted">Claude hasn't replied yet.</p>
        )}
      </div>
      <h2>{`MCP servers (${servers.length})`}</h2>
      {servers.length ? (
        <ul className="list">
          {servers.map((x) => (
            <ServerRow key={x.name} server={x} />
          ))}
        </ul>
      ) : (
        <p className="muted">None connected yet.</p>
      )}
      {signIn.length > 0 && (
        <details>
          <summary>{`${signIn.length} need sign-in`}</summary>
          <ul className="list">
            {signIn.map((x) => (
              <ServerRow key={x.name} server={x} />
            ))}
          </ul>
        </details>
      )}
      <h2>{`Skills (${s.skills.length})`}</h2>
      {s.skills.length ? (
        <ul className="list">
          {s.skills.map((name) => (
            <li key={name}>
              <span className="name" title={name}>
                {name}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="muted">None used yet.</p>
      )}
    </>
  );
}

createRoot(document.getElementById("app")!).render(<Hud />);
