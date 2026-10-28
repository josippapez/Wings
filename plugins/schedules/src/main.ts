// The Schedules main script: a timer that starts the agents that are due, and the changes the Schedules page asks for.
// Every read-then-write of the schedules goes through `queue` (main/schedules.ts), so two ticks, or a tick and an edit, never interleave.
// The "assigned" entries have their own timer and queue in main/assigned.ts.

import { errorMessage } from "wings-plugin-sdk/async";
import { registerTools } from "wings-plugin-sdk/tools";
import { ch, type State } from "./lib/messages";
import { currentSignInCode, targetsOf, pollAssigned, serveAssigned } from "./main/assigned";
import { queue, removeSchedule } from "./main/schedules";
import { readSchedules as read, readWatches, syncSections, writeSchedules as write } from "./main/store";
import { AGENTS, claim, cleanDraft, dueAt, nameOf, onStart, type Run, type Schedule } from "./lib/schedule";
import { tools } from "./tools";

/** A tick is a few comparisons, so it runs while Wings is in the background too: that is when a run is wanted. */
const TICK_MS = 30_000;

/** Starts the agent for `s`, in a new tab in its project. Never throws: a failure is the run's result. */
async function start(s: Schedule): Promise<Run> {
  try {
    const project = (await wings.projects()).find((p) => p.id === s.projectId);
    if (!project) throw new Error("The project is no longer in Wings.");
    const { paneId } = await wings.openPane({ command: AGENTS[s.agent].command, args: [s.prompt], cwd: project.path });
    return { at: Date.now(), paneId, ok: true };
  } catch (error) {
    return { at: Date.now(), ok: false, error: errorMessage(error) };
  }
}

const record = (id: string, run: Run) =>
  queue(async () => {
    const list = await read();
    if (list.some((s) => s.id === id)) await write(list.map((s) => (s.id === id ? { ...s, lastRun: run } : s)));
  });

/** Claims what is due, so it can't be due again, then starts each and tells you. */
async function tick(): Promise<void> {
  const now = Date.now();
  const due = await queue(async () => {
    const list = await read();
    const found: Schedule[] = [];
    const next = list.map((s) => {
      const at = dueAt(s, now);
      if (at === null) return s;
      found.push(s);
      return claim(s, at, now);
    });
    if (found.length) await write(next);
    return found;
  });
  for (const s of due) {
    const run = await start(s);
    await record(s.id, run);
    const project = (await wings.projects()).find((p) => p.id === s.projectId);
    wings
      .notify({
        title: run.ok ? `Started ${nameOf(s)}` : `Couldn't start ${nameOf(s)}`,
        body: run.ok ? `${AGENTS[s.agent].label} in ${project?.name ?? "its project"}` : run.error,
        projectId: project?.id,
        actions: [{ label: "Open Schedules", open: { view: "schedules" } }],
      })
      .catch(console.error);
  }
}

ch.serve("state", async (): Promise<State> => {
  const [schedules, watches, projects] = await Promise.all([read(), readWatches(), wings.projects()]);
  return {
    schedules,
    watches: await Promise.all(watches.map(async ({ seen, ...rest }) => ({ ...rest, targets: await targetsOf(rest, projects) }))),
    projects: projects.map(({ id, name }) => ({ id, name })),
    signInCode: currentSignInCode(),
  };
});
wings.onProjects(() => {
  ch.refresh("state");
  void syncSections();
});
serveAssigned();
registerTools(tools);

ch.answer("save", ({ id, draft }) =>
  queue(async () => {
    const clean = cleanDraft(draft);
    const list = await read();
    const now = Date.now();
    if (id === null) return write([...list, { ...clean, id: `${now.toString(36)}${Math.random().toString(36).slice(2, 8)}`, enabled: true, anchor: now }]);
    if (!list.some((s) => s.id === id)) throw new Error("That schedule is gone.");
    // A changed time counts from now, so it never runs for a time it was already past when you saved it.
    return write(list.map((s) => (s.id === id ? { ...s, ...clean, anchor: now } : s)));
  }),
);

ch.answer("toggle", ({ id }) =>
  queue(async () => {
    const list = await read();
    await write(list.map((s) => (s.id === id ? { ...s, enabled: !s.enabled, anchor: Date.now() } : s)));
  }),
);

ch.answer("remove", ({ id }) => removeSchedule(id));

ch.answer("runNow", async ({ id }) => {
  const s = (await read()).find((x) => x.id === id);
  if (!s) throw new Error("That schedule is gone.");
  const run = await start(s);
  await record(id, run);
  if (!run.ok) throw new Error(run.error);
});

// A schedule that came due while Wings was closed counts from now, then the timer starts.
queue(async () => {
  const now = Date.now();
  const list = await read();
  const next = list.map((s) => onStart(s, now));
  if (next.some((s, i) => s !== list[i])) await write(next);
}).then(() => {
  const run = () => tick().catch(console.error);
  void run();
  setInterval(run, TICK_MS);
  pollAssigned();
}, console.error);
void syncSections();
