import { useEffect, useState, type ReactNode } from "react";
import { CircleAlertIcon, CircleCheckIcon, CircleDashedIcon, CircleIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverDescription, PopoverHeader, PopoverTitle, PopoverTrigger } from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { api, type McpStatus } from "@/lib/api";
import { cn } from "@/lib/utils";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** `partly`: some of the setup is in place, like Claude Code's MCP entry without its status line. */
export type ConnectionState = "connected" | "partly" | "off" | "missing";

const stateIcon = { connected: CircleCheckIcon, partly: CircleDashedIcon, off: CircleIcon, missing: CircleIcon };
const stateLabel = { connected: "Connected", partly: "Partly connected", off: "Not connected", missing: "Not installed" };

/**
 * One connection as a small chip with its state; its details and buttons open in a popover, so a row of them takes
 * one line of the Plugins section. `actions` and `children` render inside the popover.
 */
export function Connection(props: { name: string; state: ConnectionState; summary: ReactNode; busy: boolean; error: string | null; actions?: ReactNode; children?: ReactNode }) {
  const Icon = props.error ? CircleAlertIcon : stateIcon[props.state];
  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button variant="outline" size="sm" className="h-7 gap-1.5 rounded-full px-2.5 text-[12px] font-normal" aria-label={`${props.name}: ${props.error ? "error" : stateLabel[props.state]}`} />
        }
      >
        {props.busy ? (
          <Spinner className="size-3.5" />
        ) : (
          <Icon aria-hidden className={cn("size-3.5", props.error ? "text-blocked" : props.state === "connected" ? "text-done" : props.state === "partly" ? "text-working" : "text-muted-foreground")} />
        )}
        {props.name}
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 gap-3 p-3.5">
        <PopoverHeader>
          <PopoverTitle className="flex items-center justify-between gap-2 text-[13px]">
            {props.name}
            <span className="text-[12px] font-normal text-muted-foreground">{stateLabel[props.state]}</span>
          </PopoverTitle>
          <PopoverDescription className="text-[12px] leading-snug">{props.summary}</PopoverDescription>
        </PopoverHeader>
        {props.children}
        {props.error && (
          <p role="alert" className="text-[12px] text-blocked">
            {props.error}
          </p>
        )}
        {props.actions && <div className="flex justify-end gap-1.5">{props.actions}</div>}
      </PopoverContent>
    </Popover>
  );
}

/** An agent CLI Wings adds its MCP server to when it starts. Another agent is one more entry in `AGENTS`. */
type Agent<S> = {
  id: string;
  name: string;
  status: () => Promise<S>;
  connect: () => Promise<void>;
  disconnect: () => Promise<void>;
  state: (s: S) => ConnectionState;
  summary: (state: ConnectionState) => string;
  /** More settings, shown in the popover while it is connected. */
  extra?: (s: S, run: (task: () => Promise<void>) => void, busy: boolean) => ReactNode;
};

const claude: Agent<McpStatus> = {
  id: "claude",
  name: "Claude Code",
  status: api.mcpStatus,
  connect: api.mcpConnect,
  disconnect: api.mcpDisconnect,
  state: (s) => (!s.claude ? "missing" : s.connected && s.statusline ? "connected" : s.connected || s.statusline ? "partly" : "off"),
  summary: (state) =>
    state === "missing"
      ? "Install Claude Code to give it plugin tools."
      : state === "connected"
        ? "Its sessions get plugin tools, and plugins get its usage numbers. Your own status line still shows."
        : "Connect it so its sessions get plugin tools and plugins get its usage numbers. Your own status line keeps showing.",
  extra: (s, run, busy) =>
    s.connected && (
      <div className="flex items-start gap-3">
        <label htmlFor="mcp-always-load" className="min-w-0 flex-1">
          <span className="block text-[13px]">Load Wings tools at session start</span>
          <span className="block text-[12px] leading-snug text-muted-foreground">
            Claude sees every plugin tool from the first message, instead of only when something tells it to look. Takes more of its context. New sessions only.
          </span>
        </label>
        <Switch id="mcp-always-load" checked={s.alwaysLoad} disabled={busy} onCheckedChange={(on) => run(() => api.mcpSetAlwaysLoad(on))} className="mt-1" />
      </div>
    ),
};

const opencode: Agent<{ opencode: boolean; connected: boolean }> = {
  id: "opencode",
  name: "OpenCode",
  status: api.opencodeMcpStatus,
  connect: api.opencodeMcpConnect,
  disconnect: api.opencodeMcpDisconnect,
  state: (s) => (!s.opencode ? "missing" : s.connected ? "connected" : "off"),
  summary: (state) =>
    state === "missing" ? "Install OpenCode to give it plugin tools." : state === "connected" ? "Its sessions get plugin tools." : "Connect it so its sessions get plugin tools.",
};

/** Binds an agent to its own status type, so agents with different status shapes share one list. */
const entry = <S,>(agent: Agent<S>) => ({ id: agent.id, render: (open: boolean, label?: string) => <AgentConnection key={agent.id} agent={agent} open={open} label={label} /> });
const AGENTS = [entry(claude), entry(opencode)];

function AgentConnection<S>({ agent, open, label }: { agent: Agent<S>; open: boolean; label?: string }) {
  const [status, setStatus] = useState<S | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) void agent.status().then(setStatus, () => {});
  }, [open, agent]);

  function run(task: () => Promise<void>) {
    setBusy(true);
    setError(null);
    void task()
      .then(agent.status)
      .then(setStatus, (e) => setError(message(e)))
      .finally(() => setBusy(false));
  }

  if (!status) return null;
  const state = agent.state(status);
  return (
    <Connection
      name={label ?? agent.name}
      state={state}
      summary={agent.summary(state)}
      busy={busy}
      error={error}
      actions={
        state !== "missing" && (
          <>
            {state !== "off" && (
              <Button variant="ghost" size="sm" className="h-7" disabled={busy} onClick={() => run(agent.disconnect)}>
                Disconnect
              </Button>
            )}
            <Button variant={state === "connected" ? "outline" : "secondary"} size="sm" className="h-7" disabled={busy} onClick={() => run(agent.connect)}>
              {state === "connected" ? "Reconnect" : "Connect"}
            </Button>
          </>
        )
      }
    >
      {agent.extra?.(status, run, busy)}
    </Connection>
  );
}

/** One agent's chip, under another name where the agent is already named, like its row in Settings > Agents. */
export function AgentConnectionChip({ id, label, open }: { id: string; label: string; open: boolean }) {
  return AGENTS.find((a) => a.id === id)?.render(open, label) ?? null;
}

/** The agents Wings connects to, one chip each. */
export function AgentConnections({ open }: { open: boolean }) {
  return AGENTS.map((agent) => agent.render(open));
}
