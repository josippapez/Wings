// The Schedules tab. Main keeps the schedules and starts the runs; this page shows them and asks for changes.

import { useEffect, useState } from "react";
import { useQuery } from "wings-plugin-sdk/react";
import { useViewArgs } from "wings-plugin-ui";
import { ch } from "../lib/messages";
import { ScheduleView } from "../ui/ScheduleView";

export const page = { place: "tab", fill: true };

/** How often "in 2 h" is worked out again. */
const CLOCK_MS = 30_000;
/** Making a worktree of a big repo takes a while, and a sign-in waits for the browser. */
const START_TIMEOUT_MS = 120_000;
const SIGN_IN_TIMEOUT_MS = 320_000;

export default function SchedulesPage() {
  const { data, error } = useQuery(ch, "state", undefined);
  // A notification's Review button opens the tab with the entry its ticket is waiting on.
  const focusWatch = useViewArgs((raw) => (typeof (raw as { waiting?: unknown })?.waiting === "string" ? (raw as { waiting: string }).waiting : null));
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
      focusWatch={focusWatch}
      onSave={(id, draft) => ch.ask("save", { id, draft })}
      onToggle={(id) => ch.ask("toggle", { id })}
      onRemove={(id) => ch.ask("remove", { id })}
      onRun={(id) => ch.ask("runNow", { id })}
      onSaveWatch={(id, draft) => ch.ask("saveWatch", { id, draft })}
      onToggleWatch={(id) => ch.ask("toggleWatch", { id })}
      onRemoveWatch={(id) => ch.ask("removeWatch", { id })}
      onCheckWatch={(id) => ch.ask("checkWatch", { id })}
      onStart={(id, ticketId) => ch.ask("startWaiting", { id, ticketId }, { timeoutMs: START_TIMEOUT_MS })}
      onDismiss={(id, ticketId) => ch.ask("dismissWaiting", { id, ticketId })}
      onSignIn={(id) => ch.ask("signIn", { id }, { timeoutMs: SIGN_IN_TIMEOUT_MS })}
      onInstall={(id) => ch.ask("install", { id }, { timeoutMs: SIGN_IN_TIMEOUT_MS })}
      onOpenUrl={(url) => wings.openUrl(url)}
    />
  );
}
