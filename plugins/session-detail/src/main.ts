// Follows each Claude session in a pane for a sidebar with its recent activity.
// It reads the newest transcript entries once per run of `claude`, then only what Claude appends. A resumed
// session's transcript also holds earlier runs, which only count for the model and context.

import { followSessions } from "wings-plugin-sdk/sessions";
import { openChanges } from "./lib/diff";
import { ch } from "./lib/messages";
import { fresh, ingest, session } from "./lib/session";
import type { SessionView } from "./lib/types";
import { snapshot } from "./lib/view";

/** Entries read for a run: the newest rows and more. A run with more than this has its older calls and token counts missed, which `partial` says. */
const BACKLOG = 500;

// The return type is written out because `follow` uses `sidebar` and `sidebar` uses `follow`.
const sidebar = ch.share("sessions", (): SessionView[] => snapshot(follow.sessions()));

const follow = followSessions({
  create: session,
  pane: (s, pane) => void Object.assign(s, { cwd: pane.cwd, project: pane.project, name: pane.session.name, state: pane.session.state }),
  startRun: (s) => void Object.assign(s, fresh()),
  async backlog(s, run) {
    const entries = await wings.transcript(s.id, ["assistant", "user", "attachment:queued_command"], { last: BACKLOG });
    // A full backlog that starts after this run began missed some of it, so its token total is a floor.
    s.partial = entries.length >= BACKLOG && (Date.parse(entries[0]?.timestamp ?? "") || 0) > run.startedAt;
    return entries;
  },
  entry: (s, entry, { thisRun }) => ingest(s, entry, thisRun),
  changed: sidebar.changed,
});

// A row opens to its detail, and a file to its diff.
ch.answer("detail", ({ sessionId, key }) => {
  const item = follow.get(sessionId)?.items.find((i) => i.key === key);
  if (!item) throw new Error("That row isn't kept any more.");
  return item;
});
ch.answer("diff", ({ sessionId, path }) => openChanges(follow.get(sessionId), path));
