// What the page and the tools share for the schedules. Every read-then-write of them goes through `queue`.

import { serial } from "wings-plugin-sdk/async";
import { readSchedules, writeSchedules } from "./store";

export const queue = serial();

export const removeSchedule = (id: string) => queue(async () => writeSchedules((await readSchedules()).filter((s) => s.id !== id)));
