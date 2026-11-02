// What the page and the tools share for the schedules. Every read-then-write of them goes through `queue`.

import { serial } from "wings-plugin-sdk/async";
import { cleanDraft, type Draft, type Schedule } from "../lib/schedule";
import { readSchedules, writeSchedules } from "./store";

export const queue = serial();

export const removeSchedule = (id: string) => queue(async () => writeSchedules((await readSchedules()).filter((s) => s.id !== id)));

/** Adds a schedule, or changes the one `id` names, and returns it as saved. Throws what the form shows. */
export const saveSchedule = (id: string | null, draft: Draft): Promise<Schedule> =>
  queue(async () => {
    const clean = cleanDraft(draft);
    const list = await readSchedules();
    const now = Date.now();
    if (id === null) {
      const added: Schedule = { ...clean, id: `${now.toString(36)}${Math.random().toString(36).slice(2, 8)}`, enabled: true, anchor: now };
      await writeSchedules([...list, added]);
      return added;
    }
    const old = list.find((s) => s.id === id);
    if (!old) throw new Error("That schedule is gone.");
    // A changed time counts from now, so it never runs for a time it was already past when you saved it.
    const saved: Schedule = { ...old, ...clean, anchor: now };
    await writeSchedules(list.map((s) => (s.id === id ? saved : s)));
    return saved;
  });
