import type { Agent, PaneInfo } from "./api.ts";

/**
 * A pane an agent opened with `open_pane`: the project and folder it opened in, and `owner`, the agent's own pane when
 * it opened as a split below it (null for a tab).
 */
export type AgentTab = { spaceId: string; cwd: string | null; owner: string | null };

/**
 * A pane the same agent opened before in the same project and folder (a split below it, or a tab) that is open, at
 * its shell prompt and not running an agent, so a new command can go there instead of another pane. Undefined when
 * there is none.
 */
export function idleAgentTab(tabs: Map<string, AgentTab>, want: AgentTab, open: Set<string>, info: Record<string, PaneInfo>, agents: Agent[]) {
  for (const [id, at] of tabs) {
    const same = at.spaceId === want.spaceId && at.cwd === want.cwd && at.owner === want.owner;
    if (same && open.has(id) && info[id]?.busy === false && !agents.some((a) => a.paneId === id)) return id;
  }
}
