import type { Agent, AgentKind } from "./api.ts";
import { isSessionKind, type SessionKind } from "./resume.ts";
import { noResume, type ResumeMeta } from "./sleep.ts";

/** The process last seen in a pane, including agents without resumable sessions. */
export type AgentRun = { kind: AgentKind; pid: number; startedAt: number };
export type PaneSession = ResumeMeta & {
  kind: SessionKind;
  name: string | null;
  live: boolean;
  args: string[];
  openedBy?: string;
  agentRun?: AgentRun;
};

/** Keep session details and plugin attribution with the agent, rather than the reusable terminal. */
export function syncPaneSession<T extends PaneSession>(meta: T, agent: Agent | undefined): T {
  if (!agent) {
    // Sleep deliberately leaves the session ready to resume in the same pane.
    if (!meta.live || meta.asleep || meta.resumeAt !== null) return meta;
    return { ...meta, sessionId: null, name: null, live: false, args: [], openedBy: undefined, agentRun: undefined };
  }

  const run = meta.agentRun;
  const sameProcess = !!run && run.kind === agent.kind && run.pid === agent.pid && run.startedAt === agent.startedAt;
  const sameSession = !!meta.sessionId && meta.kind === agent.kind && meta.sessionId === agent.sessionId;
  // A resumed process is visible before its session registry entry is ready.
  const awaitingSession = !!meta.sessionId && agent.kind === meta.kind && !agent.sessionId &&
    (!meta.live || meta.asleep || meta.resumeAt !== null || meta.resumeFailed);
  const changed = !!(
    (run && !sameProcess && !sameSession && !awaitingSession) ||
    (meta.sessionId && agent.kind !== meta.kind) ||
    (meta.sessionId && agent.sessionId && !sameSession)
  );
  // /exit is asynchronous. An unchanged idle process is still on its way to sleep.
  if (meta.asleep && sameProcess && !changed && agent.state === "idle") return meta;
  const resumable = isSessionKind(agent.kind);
  const sessionId = resumable ? (agent.sessionId ?? (changed ? null : meta.sessionId)) : null;
  const kind = resumable ? agent.kind : meta.kind;
  const name = agent.name ?? (changed ? null : meta.name);
  const args = resumable ? agent.args : [];
  const openedBy = changed ? undefined : meta.openedBy;

  if (sameProcess && meta.live && meta.sessionId === sessionId && meta.kind === kind && meta.name === name &&
      meta.openedBy === openedBy && !meta.asleep && meta.resumeAt === null && !meta.resumeFailed &&
      meta.args.length === args.length && meta.args.every((arg, i) => arg === args[i])) return meta;

  return { ...meta, sessionId, kind, name, args, openedBy, live: true, ...noResume,
    agentRun: sameProcess ? run : { kind: agent.kind, pid: agent.pid, startedAt: agent.startedAt } };
}
