// Who wrote an entry: the user, git, or the agent session behind a tool call, and how a call's context says which.

import type { ToolContext } from "wings-plugin-sdk/tools";
import { agentLabel } from "./commands";
import type { Actor, Issue, SessionRef } from "./domain";

// ---------- who wrote it ----------

/** The project a tool call came from, and its agent session as an Actor, from what Wings tells the handler. */
export function callerOf({ project, session }: ToolContext): { projectId: string; projectName: string; actor: Actor } {
  if (!project) throw new Error("Can't tell which project this call comes from. Call it from a terminal in Wings.");
  // A tool call comes from an agent, so a call with no session is a race at startup: still an agent, just without an id.
  const actor: Actor = session ? { id: session.id, ...(session.name ? { name: session.name } : {}), ...(session.kind ? { kind: session.kind } : {}) } : { id: "" };
  return { projectId: project.id, projectName: project.name, actor };
}

export const actorName = (by: Actor): string => (by === "you" ? "You" : by === "git" ? "Git" : by.name || agentLabel(by));
/** True when an agent session wrote the entry, not you or git. */
export const isAgent = (by: Actor): by is Exclude<Actor, "you" | "git"> => by !== "you" && by !== "git";

/** The session that touched the issue last, or null. Resume picks this one. */
export const lastSession = (issue: Issue): SessionRef | null => issue.sessions.reduce<SessionRef | null>((a, s) => (!a || s.seen > a.seen ? s : a), null);
