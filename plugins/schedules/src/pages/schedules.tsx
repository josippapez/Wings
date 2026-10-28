// The Schedules tab. Main keeps the schedules and starts the runs; this page shows them and asks for changes.

import { useEffect, useState } from "react";
import { useQuery } from "wings-plugin-sdk/react";
import { ch } from "../lib/messages";
import { ScheduleView } from "../ui/ScheduleView";

export const page = { place: "tab", fill: true };

/** How often "in 2 h" is worked out again. */
const CLOCK_MS = 30_000;

export default function SchedulesPage() {
  const { data, error } = useQuery(ch, "state", undefined);
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
      onSave={(id, draft) => ch.ask("save", { id, draft })}
      onToggle={(id) => ch.ask("toggle", { id })}
      onRemove={(id) => ch.ask("remove", { id })}
      onRun={(id) => ch.ask("runNow", { id })}
    />
  );
}
