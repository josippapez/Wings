// The commands that start an agent in a pane, one entry per agent CLI. Adding an agent is an entry in AGENTS and its
// two commands in the manifest's permissions.panes.

import { KEY_PATTERN, type AgentKind, type Issue } from "./domain";

interface AgentCli {
  /** What the UI calls it. */
  label: string;
  /** The shape of its session ids: they're typed into a shell, so nothing else is accepted. */
  sessionId: RegExp;
  /** Continues a session, as `openPane`'s `command`. */
  resume: (sessionId: string) => string;
  /** Starts it with a first prompt, as `openPane`'s `command`; the prompt goes in `args`. */
  start: string;
}

export const AGENTS: Record<AgentKind, AgentCli> = {
  claude: { label: "Claude Code", sessionId: /^[0-9A-Za-z-]{8,64}$/, resume: (id) => `claude --resume ${id}`, start: "claude" },
  // `opencode --help`: --session continues a session, --prompt is the first message.
  opencode: { label: "OpenCode", sessionId: /^ses_[A-Za-z0-9]{8,64}$/, resume: (id) => `opencode --session ${id}`, start: "opencode --prompt" },
};
export const AGENT_KINDS = Object.keys(AGENTS) as AgentKind[];

/** A session's agent. One recorded before sessions had a kind is Claude Code's, the only agent Wings followed then. */
export const kindOf = (s: { kind?: string }): AgentKind => (s.kind && Object.hasOwn(AGENTS, s.kind) ? (s.kind as AgentKind) : "claude");
export const agentLabel = (s: { kind?: string }): string => AGENTS[kindOf(s)].label;

/**
 * Pane commands are typed into the user's shell: Wings takes command words of letters, digits and -_./:=@%+, and
 * quotes each `args` value as one word itself, spaces and all. Nothing here is user text: a key and a session id are
 * checked first.
 */
export const PLAIN_WORD = /^[A-Za-z0-9_.\/:=@%+,-]+$/;
export function resumeCommand(session: { id: string; kind?: string }): string {
  const agent = AGENTS[kindOf(session)];
  if (!agent.sessionId.test(session.id)) throw new Error(`That isn't a ${agent.label} session id.`);
  return agent.resume(session.id);
}
/**
 * The agent with its first prompt for `issue`, as `openPane`'s `command` and `args`. It takes the issue, not a key: a
 * tab can be opened with another spelling of the key, and the prompt must carry the one the issue has.
 */
export function agentCommand(issue: Pick<Issue, "key">, kind: AgentKind = "claude"): { command: string; args: string[] } {
  if (!KEY_PATTERN.test(issue.key)) throw new Error("That isn't an issue key.");
  const prompt = `Continue issue ${issue.key}. Read it with tasks__get, do the work, and log progress with tasks__log and the status with tasks__update as you go.`;
  return { command: AGENTS[kind].start, args: [prompt] };
}
