import type { Agent } from "./api.ts";

/**
 * The agent the "next agent that needs you" shortcut goes to: blocked ones first, then finished ones you
 * haven't looked at, each in list order. Repeating it walks the whole list and wraps; from a pane that isn't
 * in it, it starts at the top. `back` walks the other way.
 */
export function nextNeedingYou(agents: Agent[], focusedPaneId: string | null, back = false): Agent | null {
  const queue = [...agents.filter((a) => a.state === "blocked"), ...agents.filter((a) => a.state === "done")];
  if (queue.length === 0) return null;
  const at = queue.findIndex((a) => a.paneId === focusedPaneId);
  if (at === -1) return queue[back ? queue.length - 1 : 0];
  return queue[(at + (back ? -1 : 1) + queue.length) % queue.length];
}

/** "42s", "7m", "1h 5m": how long an agent has been in its state. Under a second reads as "0s". */
export function elapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** What a pane's icon shows: the agent's own mark while it is idle, its live state dot otherwise (the same for every agent kind). */
export function paneGlyph(agent: Agent | undefined): "shell" | "claude" | "opencode" | "state" {
  if (!agent) return "shell";
  return agent.state === "idle" ? agent.kind : "state";
}
