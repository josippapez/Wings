// The saved schedules and "assigned" entries. Every write tells the page and redraws the project sections, the rows
// under each project in the sidebar that show which projects have something set up.

import { describeWhen, nameOf, AGENTS, type Schedule } from "../lib/schedule";
import { SOURCE_LABELS, watchName, type Watch } from "../lib/assigned";
import { ch } from "../lib/messages";

const SCHEDULES = "schedules";
const WATCHES = "assigned";
export const SECTION_ID = "schedules";

export const readSchedules = async (): Promise<Schedule[]> => {
  const raw = await wings.storage.get(SCHEDULES);
  return Array.isArray(raw) ? (raw as Schedule[]) : [];
};

export const readWatches = async (): Promise<Watch[]> => {
  const raw = await wings.storage.get(WATCHES);
  return Array.isArray(raw) ? (raw as Watch[]) : [];
};

export async function writeSchedules(list: Schedule[]): Promise<void> {
  await wings.storage.set(SCHEDULES, list);
  ch.refresh("state");
  void syncSections();
}

export async function writeWatches(list: Watch[]): Promise<void> {
  await wings.storage.set(WATCHES, list);
  ch.refresh("state");
  void syncSections();
}

/** Shows each project the schedules and "assigned" entries that run in it, and removes the section from the rest. */
export async function syncSections(): Promise<void> {
  const [schedules, watches, projects] = await Promise.all([readSchedules(), readWatches(), wings.projects()]);
  for (const { id } of projects) {
    const rows: WingsProjectSectionRow[] = [
      ...schedules
        .filter((s) => s.projectId === id)
        .map((s) => ({
          id: `s:${s.id}`,
          label: nameOf(s),
          detail: describeWhen(s.when),
          ...(s.enabled ? {} : { badge: { text: "Paused" } }),
          open: { view: "schedules" },
        })),
      ...watches
        .filter((w) => w.projectId === id)
        .map((w) => ({
          id: `w:${w.id}`,
          label: watchName(w),
          detail: `${AGENTS[w.agent].label} · ${w.source ? SOURCE_LABELS[w.source.kind] : "Checking"}`,
          ...(w.waiting.length ? { badge: { text: `${w.waiting.length} waiting`, tone: "info" as const } } : w.enabled ? {} : { badge: { text: "Paused" } }),
          open: { view: "schedules", args: { waiting: w.id } },
        })),
    ];
    await wings.setProjectSection(id, SECTION_ID, rows.length ? rows : null).catch(console.error);
  }
}
