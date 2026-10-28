// The Schedules tab: a list of schedules (running, paused, one that failed and one that never ran), empty, loading, a
// failed load, adding, editing, asking to delete, and a save the form refuses.
import { ScheduleView, type ViewProps } from "../src/ui/ScheduleView";
import type { Schedule } from "../src/lib/schedule";
import { Steps } from "./_steps";

export { page } from "../src/pages/schedules";

const now = Date.now();
const H = 3_600_000;
const projects = [
  { id: "p1", name: "acme-web" },
  { id: "p2", name: "acme-api" },
  { id: "p3", name: "docs" },
];

const schedules: Schedule[] = [
  {
    id: "a",
    name: "Morning review",
    projectId: "p1",
    agent: "claude",
    prompt: "Read yesterday's merged pull requests and list anything that needs a follow-up.",
    when: { kind: "daily", time: "09:00", days: [1, 2, 3, 4, 5] },
    enabled: true,
    anchor: now,
    lastRun: { at: now - 26 * H, paneId: "x1", ok: true },
  },
  {
    id: "b",
    name: "",
    projectId: "p2",
    agent: "opencode",
    prompt: "Run the test suite and, if anything is flaky, open a fix for the worst offender.",
    when: { kind: "every", hours: 4 },
    enabled: true,
    anchor: now - H,
    lastRun: { at: now - H, ok: false, error: "The project is no longer in Wings." },
  },
  {
    id: "c",
    name: "Docs sweep",
    projectId: "p3",
    agent: "claude",
    prompt: "Check the docs for links that no longer work.",
    when: { kind: "daily", time: "18:30", days: [5] },
    enabled: false,
    anchor: now - 50 * H,
  },
];

const noop = () => Promise.resolve();
const base: ViewProps = { state: { schedules, projects }, error: null, now, onSave: noop, onToggle: noop, onRemove: noop, onRun: noop };

export const states = {
  list: { element: <ScheduleView {...base} /> },
  empty: { element: <ScheduleView {...base} state={{ schedules: [], projects }} /> },
  loading: { element: <ScheduleView {...base} state={null} /> },
  error: { element: <ScheduleView {...base} state={null} error="Wings didn't answer. Try again." /> },
  add: { element: <Steps steps={[{ click: "Add schedule" }]}><ScheduleView {...base} /></Steps> },
  edit: { element: <Steps steps={[{ click: "Edit Morning review" }]}><ScheduleView {...base} /></Steps> },
  "edit-every": { element: <Steps steps={[{ click: "Edit Run the test suite and, if anything is flaky, o…" }]}><ScheduleView {...base} /></Steps> },
  delete: { element: <Steps steps={[{ click: "Delete Docs sweep" }]}><ScheduleView {...base} /></Steps> },
  "save-failed": {
    element: (
      <Steps steps={[{ click: "Add schedule" }, { click: "Add schedule" }]}>
        <ScheduleView {...base} onSave={() => Promise.reject(new Error("Write what the agent should do."))} />
      </Steps>
    ),
  },
};
