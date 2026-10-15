// The HUD sidebar page: usage limits and memory, then one session's context, MCP servers and skills. The plugin's
// main script keeps the state and shares it here.

import { useState } from "react";
import { useShared } from "wings-plugin-sdk/react";
import { Card, Disclosure, Dot, Empty, Gauge, Heading, List, ListItem, Loading, Row, Select, Stack, Text } from "wings-plugin-ui";

import { gb, tokens } from "../lib/format";
import { CONTEXT_CRITICAL, CONTEXT_WARN, LIMIT_CRITICAL, LIMIT_WARN } from "../lib/levels";
import { ch } from "../lib/messages";
import "./sidebar.css";
import type { Server, ServerState } from "../lib/types";

export const page = { place: "sidebar" };

const limitLevels: [number, number] = [LIMIT_WARN, LIMIT_CRITICAL];
const contextLevels: [number, number] = [CONTEXT_WARN, CONTEXT_CRITICAL];

const states = { connected: "Connected", failed: "Failed", connecting: "Connecting", "sign-in": "Needs sign-in" };
const dots: Record<ServerState, "danger" | "muted" | undefined> = { connected: undefined, failed: "danger", connecting: "muted", "sign-in": "muted" };

function ServerRow({ server }: { server: Server }) {
  const tools = server.tools ? `, ${server.tools} ${server.tools === 1 ? "tool" : "tools"}` : "";
  return (
    <ListItem title={`${server.name}: ${states[server.state]}${tools}${server.error ? `\n${server.error}` : ""}`}>
      <Stack gap="none" grow>
        <Row>
          <Dot tone={dots[server.state]} />
          <Text grow truncate>
            {server.name}
          </Text>
          {server.state === "failed" ? (
            <Text tone="danger">Failed</Text>
          ) : server.state === "connecting" ? (
            <Text tone="muted">Connecting</Text>
          ) : null}
          {server.lastFailed ? <Text tone="warn">Last call failed</Text> : server.used ? <Text tone="muted">Used</Text> : null}
        </Row>
        {server.state === "failed" && server.error ? (
          <Text tone="danger" size="tiny" truncate className="server-error">
            {server.error}
          </Text>
        ) : null}
      </Stack>
    </ListItem>
  );
}

export default function Hud() {
  const hud = useShared(ch, "hud");
  /** The session you picked; otherwise the first. */
  const [picked, setPicked] = useState<string | null>(null);

  if (hud === null) return <Loading />;
  const { overview: o, sessions } = hud;
  const usage = (
    <>
      <Heading>Usage</Heading>
      <Card>
        {o.fiveHour && <Gauge label="5-hour limit" percent={o.fiveHour.percent} note={o.fiveHour.resets && `Resets in ${o.fiveHour.resets}`} levels={limitLevels} />}
        {o.sevenDay && <Gauge label="Weekly limit" percent={o.sevenDay.percent} note={o.sevenDay.resets && `Resets in ${o.sevenDay.resets}`} levels={limitLevels} />}
        {!o.statusline && (
          <Text as="p" tone="muted">
            Your 5-hour and weekly limits show after Claude Code's next reply, on Pro and Max plans. If they don't, connect Claude Code in Plugins.
          </Text>
        )}
        {o.memory && <Gauge label="Memory" percent={o.memory.percent} note={`${gb(o.memory.used)} of ${gb(o.memory.total)} in use`} levels={limitLevels} />}
      </Card>
    </>
  );

  if (sessions.length === 0) {
    return (
      <>
        {usage}
        <Empty title="No Claude session in your panes." description="Run claude in a pane to see its context, MCP servers and skills." />
      </>
    );
  }
  const s = sessions.find((x) => x.id === picked) ?? sessions[0];
  const servers = s.servers.filter((x) => x.state !== "sign-in");
  const signIn = s.servers.filter((x) => x.state === "sign-in");
  return (
    <>
      {usage}
      {sessions.length > 1 && <Select label="Session" value={s.id} onChange={setPicked} options={sessions.map((x) => ({ value: x.id, label: `${x.project} · ${x.name ?? x.id.slice(0, 8)}` }))} />}
      <Heading>Context</Heading>
      <Card>
        {s.context ? (
          <Gauge
            label="Context window"
            percent={s.context.percent}
            note={[`${tokens(s.context.tokens)} of ${tokens(s.context.window)} tokens`, s.context.cache && `prompt cache ${s.context.cache.toLowerCase()}`].filter(Boolean).join(" · ")}
            levels={contextLevels}
          />
        ) : (
          <Text as="p" tone="muted">
            Claude hasn't replied yet.
          </Text>
        )}
      </Card>
      <Heading>{`MCP servers (${servers.length})`}</Heading>
      {servers.length ? (
        <List>
          {servers.map((x) => (
            <ServerRow key={x.name} server={x} />
          ))}
        </List>
      ) : (
        <Text as="p" tone="muted">
          None connected yet.
        </Text>
      )}
      {signIn.length > 0 && (
        <Disclosure summary={`${signIn.length} need sign-in`}>
          <List>
            {signIn.map((x) => (
              <ServerRow key={x.name} server={x} />
            ))}
          </List>
        </Disclosure>
      )}
      <Heading>{`Skills (${s.skills.length})`}</Heading>
      {s.skills.length ? (
        <List>
          {s.skills.map((name) => (
            <ListItem key={name}>
              <Text grow truncate title={name}>
                {name}
              </Text>
            </ListItem>
          ))}
        </List>
      ) : (
        <Text as="p" tone="muted">
          None used yet.
        </Text>
      )}
    </>
  );
}
