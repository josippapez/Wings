// The Schedules tab: a list of schedules and "assigned" entries (running, paused, one that failed, one that never ran, one
// that checked fine and one whose CLI is signed out), tickets waiting for you, empty, loading, a failed load, adding either
// kind, editing, asking to delete, a save the form refuses, and a Start that is refused for lack of disk space.
import { ScheduleView, type ViewProps } from "../src/ui/ScheduleView";
import type { Schedule } from "../src/lib/schedule";
import type { WatchView } from "../src/lib/messages";
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

const PROMPT = "Work on {url}: {title}. Read the ticket first, then make the changes on this branch.";
const watches: WatchView[] = [
  {
    id: "w1",
    targets: [projects[0]],
    projectId: "p1",
    agent: "claude",
    prompt: PROMPT,
    minFreeGb: 10,
    enabled: true,
    primed: true,
    waiting: [],
    source: { kind: "github", host: "github.com", path: "acme/web" },
    lastCheck: { at: now - 3 * 60_000, ok: true, count: 4 },
    lastRun: { at: now - 5 * H, paneId: "x2", ok: true },
  },
  {
    id: "w2",
    targets: [projects[1]],
    projectId: "p2",
    agent: "opencode",
    prompt: PROMPT,
    minFreeGb: 20,
    enabled: true,
    primed: true,
    waiting: [],
    source: { kind: "azure", org: "https://dev.azure.com/acme", project: "Shop" },
    lastCheck: { at: now - 2 * 60_000, ok: false, error: "Sign in to Azure DevOps to see what is assigned to you.", problem: "signed-out" },
  },
];

const waitingWatches: WatchView[] = [
  {
    ...watches[0],
    waiting: [
      { id: "412", title: "Checkout fails when the cart has a gift card", url: "https://github.com/acme/web/issues/412", at: now - 4 * 60_000 },
      { id: "415", title: "Add a dark mode toggle to settings", url: "https://github.com/acme/web/issues/415", at: now - 60_000 },
    ],
  },
  { ...watches[1], lastCheck: { at: now - 2 * 60_000, ok: true, count: 1 }, waiting: [] },
];

const azure = { kind: "azure" as const, org: "https://dev.azure.com/acme", project: "Shop" };
const ticket = (id: string, title: string, state: string, type: string, branch?: string) => ({
  id, title, state, type, url: `https://dev.azure.com/acme/Shop/_workitems/edit/${id}`,
  ...(branch ? { branch: { name: branch, project: "acme-api" } } : {}),
});
// Six assigned, as Azure DevOps names their states: two of them are not started. Three have a branch, one has an issue in progress, and one was started from here.
const assigned = [
  ticket("1204", "Review the order export rewrite", "In Code Review", "Tech Story", "feature/1204-order-export-rewrite"),
  ticket("1198", "Move the invoice job to the new queue", "In Progress", "User Story", "origin/feature/1198-invoice-queue"),
  ticket("1211", "Show the delivery estimate on the cart", "Ready for Development", "User Story"),
  ticket("1187", "Fix the broken filter chips on mobile", "Ready for Peer Review/QA", "FE Task", "feature/1187-filter-chips"),
  ticket("1190", "Align the footer links with the new design", "Ready for Peer Review/QA", "FE Task"),
  { ...ticket("1215", "Add retry to the shipping rates call", "New", "Tech Story"), issue: { key: "API-42", status: "in_progress", project: "acme-api" } },
];
const assignedWatches: WatchView[] = [
  { ...watches[0], projectId: "p2", agent: "claude", source: azure, lastCheck: { at: now - 2 * 60_000, ok: true, count: 6 }, states: ["Ready for Development", "New"], tickets: assigned, started: { "1190": now - 12 * 60_000 }, targets: [projects[1], projects[0]] },
];

const noop = () => Promise.resolve();
const base: ViewProps = {
  state: { schedules, watches, projects, signInCode: null },
  error: null,
  now,
  onSave: noop,
  onToggle: noop,
  onRemove: noop,
  onRun: noop,
  onSaveWatch: noop,
  onToggleWatch: noop,
  onRemoveWatch: noop,
  onCheckWatch: noop,
  onStart: noop,
  onDismiss: noop,
  onSignIn: noop,
  onInstall: noop,
  onOpenUrl: noop,
};
const assignedState = { schedules: [], watches: assignedWatches, projects, signInCode: null };
const waitingState = { schedules, watches: waitingWatches, projects, signInCode: null };

export const states = {
  list: { element: <ScheduleView {...base} /> },
  empty: { element: <ScheduleView {...base} state={{ schedules: [], watches: [], projects, signInCode: null }} /> },
  "other-project": { element: <ScheduleView {...base} here="p3" /> },
  "this-project": { element: <ScheduleView {...base} here="p1" /> },
  loading: { element: <ScheduleView {...base} state={null} /> },
  error: { element: <ScheduleView {...base} state={null} error="Wings didn't answer. Try again." /> },
  add: { element: <Steps steps={[{ click: "Add entry" }]}><ScheduleView {...base} /></Steps> },
  waiting: { element: <ScheduleView {...base} state={waitingState} /> },
  assigned: { element: <Steps steps={[{ click: "Assigned to you (6)" }]}><ScheduleView {...base} state={assignedState} /></Steps> },
  "inbox-started": { element: <ScheduleView {...base} state={assignedState} inbox={{ entry: "w1", ticket: "1211", status: "started" }} /> },
  "inbox-failed": { element: <ScheduleView {...base} state={assignedState} inbox={{ entry: "w1", ticket: "1211", status: "failed", error: "Only 6.2 GB is free on Macintosh HD, and you asked for at least 10 GB. Free up some space, then start it again." }} /> },
  "edit-assigned": { element: <Steps steps={[{ click: "Edit When something is assigned to me in acme-api" }]}><ScheduleView {...base} state={assignedState} /></Steps> },
  "add-assigned": { element: <Steps steps={[{ click: "Add entry" }, { click: "When something is assigned to me" }]}><ScheduleView {...base} /></Steps> },
  "not-enough-disk": {
    element: (
      <Steps steps={[{ click: "Start Checkout fails when the cart has a gift card" }]}>
        <ScheduleView
          {...base}
          state={waitingState}
          onStart={() => Promise.reject(new Error("Only 6.2 GB is free on Macintosh HD, and you asked for at least 10 GB. Free up some space, then start it again."))}
        />
      </Steps>
    ),
  },
  edit: { element: <Steps steps={[{ click: "Edit Morning review" }]}><ScheduleView {...base} /></Steps> },
  "edit-every": { element: <Steps steps={[{ click: "Edit Run the test suite and, if anything is flaky, o…" }]}><ScheduleView {...base} /></Steps> },
  delete: { element: <Steps steps={[{ click: "Delete Docs sweep" }]}><ScheduleView {...base} /></Steps> },
  "save-failed": {
    element: (
      <Steps steps={[{ click: "Add entry" }, { click: "Add entry" }]}>
        <ScheduleView {...base} onSave={() => Promise.reject(new Error("Write what the agent should do."))} />
      </Steps>
    ),
  },
};
