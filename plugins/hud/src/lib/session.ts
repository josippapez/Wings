import { text } from "./format";
import type { Server, ServerState } from "./types";

/** Tools a claude.ai connector offers until you sign in to it. */
const SIGN_IN_TOOLS = new Set(["authenticate", "complete_authentication"]);

/** What one `claude` process knows. A new process reconnects to every MCP server, so this starts over. */
interface Run {
  tokens: number | null;
  window: number | null;
  /** Connected servers and their tools. */
  tools: Map<string, Set<string>>;
  /** Servers that failed to connect, with the error. */
  failed: Map<string, string>;
  pending: Set<string>;
  /** Servers whose tools Claude called. */
  used: Set<string>;
  /** Whether each server's last call failed. */
  lastFailed: Map<string, boolean>;
  /** Tool call id to server. */
  calls: Map<string, string>;
  skills: Set<string>;
}

export interface Session extends Run {
  id: string;
  /** The pane that runs it, which shows its strip. */
  paneId: string;
  project: string;
  name: string | null;
  /** When this run's `claude` started, in ms. What Claude Code reported before that isn't this run's. */
  startedAt: number;
}

const newRun = (): Run => ({
  tokens: null,
  window: null,
  tools: new Map(),
  failed: new Map(),
  pending: new Set(),
  used: new Set(),
  lastFailed: new Map(),
  calls: new Map(),
  skills: new Set(),
});

export const session = (id: string): Session => ({ id, paneId: "", project: "", name: null, startedAt: 0, ...newRun() });

/** A new `claude` process runs the session, so what the last one knew is dropped. */
export const startRun = (s: Session, startedAt: number) => void Object.assign(s, newRun(), { startedAt });

/** A message's content blocks. A transcript can hold a null among them, so each is checked. */
const blocksOf = (message: TranscriptMessage | undefined): (TranscriptBlock | null)[] => (Array.isArray(message?.content) ? message.content : []);

/** `mcp__plugin_dev-core_chrome-devtools__click` is server `plugin_dev-core_chrome-devtools`, tool `click`. */
function mcpTool(name: unknown) {
  const m = /^mcp__(.+?)__(.+)$/.exec(String(name));
  return m ? { server: m[1], tool: m[2] } : null;
}

/** `thisRun` is false for an entry from before this `claude` started, which a resumed session's transcript also holds. */
export function ingest(s: Session, entry: TranscriptEntry, thisRun: boolean) {
  if (entry.type === "attachment") {
    const a = entry.attachment ?? {};
    if (a.type === "model") {
      // `claude-opus-5-5[1m]` is the 1M-token window; otherwise Claude Code's default of 200k.
      s.window = /\[1m\]$/i.test(String(a.identity?.modelId ?? "")) ? 1_000_000 : 200_000;
    } else if (a.type === "deferred_tools_delta" && thisRun) {
      for (const name of a.addedNames ?? []) {
        const t = mcpTool(name);
        if (t) s.tools.set(t.server, (s.tools.get(t.server) ?? new Set()).add(t.tool));
      }
      for (const name of a.removedNames ?? []) {
        const t = mcpTool(name);
        if (!t) continue;
        s.tools.get(t.server)?.delete(t.tool);
        if (s.tools.get(t.server)?.size === 0) s.tools.delete(t.server);
      }
      // These two list every server in that state now, not what changed.
      if (Array.isArray(a.failedMcpServers)) s.failed = new Map(a.failedMcpServers.map((f) => [text(f?.name, 64), text(f?.errorCode || f?.error, 80)]));
      if (Array.isArray(a.pendingMcpServers)) s.pending = new Set(a.pendingMcpServers.map((n) => text(n, 64)));
    } else if (a.type === "invoked_skills") {
      for (const skill of a.skills ?? []) if (skill?.name) s.skills.add(text(skill.name, 64));
    }
    return;
  }
  if (entry.type === "assistant") {
    const u = entry.message?.usage;
    // What Claude Code counts as context: everything sent in, cached or not.
    if (u) s.tokens = (u.input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0);
    for (const block of blocksOf(entry.message)) {
      if (block?.type !== "tool_use") continue;
      if (block.name === "Skill" && block.input?.skill) s.skills.add(text(block.input.skill, 64));
      const t = mcpTool(block.name);
      if (t) {
        s.used.add(t.server);
        s.calls.set(block.id!, t.server);
      }
    }
  } else if (entry.type === "user") {
    for (const block of blocksOf(entry.message)) {
      const server = block?.type === "tool_result" ? s.calls.get(block.tool_use_id!) : undefined;
      if (!server) continue;
      s.lastFailed.set(server, block!.is_error === true);
      s.calls.delete(block!.tool_use_id!);
    }
  }
}

/** Every server this run knows about, failed ones first. */
export function servers(s: Session): Server[] {
  const list: Omit<Server, "used" | "lastFailed">[] = [];
  for (const [name, tools] of s.tools) {
    const signIn = [...tools].every((t) => SIGN_IN_TOOLS.has(t));
    list.push({ name, state: signIn ? "sign-in" : "connected", tools: tools.size });
  }
  for (const [name, error] of s.failed) if (!s.tools.has(name)) list.push({ name, state: "failed", error });
  for (const name of s.pending) if (!s.tools.has(name) && !s.failed.has(name)) list.push({ name, state: "connecting" });
  // Without tool search Claude Code records no tool lists, so a server Claude used is the only sign of it.
  for (const name of s.used) if (!list.some((x) => x.name === name)) list.push({ name, state: "connected", tools: null });
  const order: Record<ServerState, number> = { failed: 0, connecting: 1, connected: 2, "sign-in": 3 };
  return list
    .map((x) => ({ ...x, used: s.used.has(x.name), lastFailed: s.lastFailed.get(x.name) === true }))
    .sort((a, b) => order[a.state] - order[b.state] || Number(b.used) - Number(a.used) || a.name.localeCompare(b.name));
}
