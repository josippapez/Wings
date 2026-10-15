// Who wrote an entry: the user, git, or the Claude session behind a tool call, and how a call's pane says which.

import type { Actor, Issue, SessionRef } from "./domain";

// ---------- who wrote it ----------

/** What the pane list says about the calling terminal: its project, and the Claude session as an Actor. */
export interface PaneLike {
  paneId: string;
  project: string;
  projectId: string;
  session: { sessionId: string; name: string | null } | null;
}
export function resolvePane(panes: PaneLike[], paneId: string | null): { projectId: string; projectName: string; actor: Actor } {
  const pane = panes.find((p) => p.paneId === paneId);
  if (!pane) throw new Error("Can't tell which project this call comes from. Call it from a terminal in Wings.");
  // A tool call comes from Claude, so a pane with no session is a race at startup: still an agent, just without an id.
  const actor: Actor = pane.session ? { id: pane.session.sessionId, ...(pane.session.name ? { name: pane.session.name } : {}) } : { id: "" };
  return { projectId: pane.projectId, projectName: pane.project, actor };
}

export const actorName = (by: Actor): string => (by === "you" ? "You" : by === "git" ? "Git" : by.name || "Claude");
/** The Claude session of an entry's author, or null for you and git. */
export const isClaude = (by: Actor): by is { id: string; name?: string } => by !== "you" && by !== "git";

/** The session that touched the issue last, or null. Resume picks this one. */
export const lastSession = (issue: Issue): SessionRef | null => issue.sessions.reduce<SessionRef | null>((a, s) => (!a || s.seen > a.seen ? s : a), null);
