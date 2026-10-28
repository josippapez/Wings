// The Schedules tab. Main keeps the schedules and starts the runs; this page shows them and asks for changes.

import { useEffect, useRef, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { useQuery } from "wings-plugin-sdk/react";
import { useProject, useViewArgs } from "wings-plugin-ui";
import { ch } from "../lib/messages";
import { ScheduleView, type InboxStart } from "../ui/ScheduleView";

export const page = { place: "tab", fill: true };

/** How often "in 2 h" is worked out again. */
const CLOCK_MS = 30_000;
/** Making a worktree of a big repo takes a while, and a sign-in waits for the browser. */
const START_TIMEOUT_MS = 120_000;
const SIGN_IN_TIMEOUT_MS = 320_000;

const loadBranches = (projectId: string) => ch.ask("branches", { projectId });

export default function SchedulesPage() {
  const { data, error } = useQuery(ch, "state", undefined);
  const here = useProject()?.id ?? null;
  // A notification's Review button opens the tab with the entry its ticket is waiting on, and its Start button with the ticket to start.
  // A tab that is open takes new args by reloading, and the same args only bring it to the front, so this runs once for each.
  const args = useViewArgs((raw) => {
    const { waiting, start } = (raw ?? {}) as { waiting?: unknown; start?: { entry?: unknown; ticket?: unknown } };
    return {
      waiting: typeof waiting === "string" ? waiting : null,
      start: typeof start?.entry === "string" && typeof start.ticket === "string" ? { entry: start.entry, ticket: start.ticket } : null,
    };
  });
  const focusWatch = args?.waiting ?? args?.start?.entry ?? null;
  const [inbox, setInbox] = useState<InboxStart | null>(null);
  const handled = useRef<string | null>(null);
  useEffect(() => {
    const start = args?.start;
    if (!start || handled.current === `${start.entry}:${start.ticket}`) return;
    handled.current = `${start.entry}:${start.ticket}`;
    setInbox({ ...start, status: "starting" });
    ch.ask("startTicket", { id: start.entry, ticketId: start.ticket }, { timeoutMs: START_TIMEOUT_MS }).then(
      () => setInbox({ ...start, status: "started" }),
      (e) => setInbox({ ...start, status: "failed", error: errorMessage(e) }),
    );
  }, [args]);
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => clearInterval(timer);
  }, []);
  return (
    <ScheduleView
      state={data}
      error={error?.message ?? null}
      now={now}
      here={here}
      focusWatch={focusWatch}
      inbox={inbox}
      onSave={(id, draft) => ch.ask("save", { id, draft })}
      onToggle={(id) => ch.ask("toggle", { id })}
      onRemove={(id) => ch.ask("remove", { id })}
      onRun={(id) => ch.ask("runNow", { id })}
      onSaveWatch={(id, draft) => ch.ask("saveWatch", { id, draft })}
      onToggleWatch={(id) => ch.ask("toggleWatch", { id })}
      onRemoveWatch={(id) => ch.ask("removeWatch", { id })}
      onCheckWatch={(id) => ch.ask("checkWatch", { id })}
      onStart={(id, ticketId, force, projectId) => ch.ask("startTicket", { id, ticketId, force, projectId }, { timeoutMs: START_TIMEOUT_MS })}
      onDismiss={(id, ticketId) => ch.ask("dismissWaiting", { id, ticketId })}
      onSignIn={(id) => ch.ask("signIn", { id }, { timeoutMs: SIGN_IN_TIMEOUT_MS })}
      onInstall={(id) => ch.ask("install", { id }, { timeoutMs: SIGN_IN_TIMEOUT_MS })}
      onOpenUrl={(url) => wings.openUrl(url)}
      loadBranches={loadBranches}
    />
  );
}
