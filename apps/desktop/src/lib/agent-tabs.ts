import type { Agent, PaneInfo } from "./api.ts";

/**
 * A pane an agent opened with `open_pane`: the project and folder it opened in, and `owner`, the agent's own pane when
 * it opened as a split below it (null for a tab).
 */
export type AgentTab = { spaceId: string; cwd: string | null; owner: string | null };

/**
 * A pane the same agent opened before in the same project (a split below it, or a tab) that is open, at its shell
 * prompt and not running an agent, so a new command can go there instead of another pane. One already in the wanted
 * folder comes first; another needs a `cd` first. Undefined when there is none.
 */
export function idleAgentTab(tabs: Map<string, AgentTab>, want: AgentTab, open: Set<string>, info: Record<string, PaneInfo>, agents: Agent[]) {
  const idle = [...tabs].filter(([id, at]) => at.spaceId === want.spaceId && at.owner === want.owner && open.has(id) && info[id]?.busy === false && !agents.some((a) => a.paneId === id));
  return (idle.find(([, at]) => at.cwd === want.cwd) ?? idle[0])?.[0];
}
