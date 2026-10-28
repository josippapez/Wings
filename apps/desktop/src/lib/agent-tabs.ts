import type { Agent, PaneInfo } from "./api.ts";

/** A pane an agent opened with `open_pane`: the project and folder it opened in. */
export type AgentTab = { spaceId: string; cwd: string | null };

/**
 * The tab an agent opened before in the same project and folder that is open, at its shell prompt and not running an
 * agent, so a new command can go there instead of another tab. Undefined when there is none.
 */
export function idleAgentTab(tabs: Map<string, AgentTab>, want: AgentTab, open: Set<string>, info: Record<string, PaneInfo>, agents: Agent[]) {
  for (const [id, at] of tabs) {
    if (at.spaceId === want.spaceId && at.cwd === want.cwd && open.has(id) && info[id]?.busy === false && !agents.some((a) => a.paneId === id)) return id;
  }
}
