// The only writer of the issues. Pages ask for operations and tools run the same ones, one at a time, each against what
// storage holds at that moment. After a change it tells the pages and refreshes the project's section in the sidebar.

import { serial } from "wings-plugin-sdk/async";
import type { Actor, Issue, Op } from "../lib/domain";
import { ch } from "../lib/messages";
import { applyOp } from "../lib/ops";
import { migrateLegacy, readStore, writeStore } from "../lib/storage";
import { pushSection } from "./section";

/** Writes run one after another, so a read-then-write never interleaves with another. */
const queue = serial();

/** Applies `op` as `by` to a project's issues, saves it, and tells the pages. An op that changes nothing saves and sends nothing. */
export function write(projectId: string, projectName: string, op: Op, by: Actor): Promise<{ issue: Issue; changed: boolean }> {
  return queue(async () => {
    const { store, issue, changed } = applyOp(await readStore(projectId, projectName), op, by);
    if (changed) {
      await writeStore(projectId, store);
      await ch.tell("issues", { projectId }).catch(console.error);
      await pushSection(projectId, store.issues);
    }
    return { issue, changed };
  });
}

/**
 * Picks up a project Wings listed: saves older data in the current format, then fills its section from storage. It queues
 * with the writes, so it never puts older rows over newer ones.
 */
export const loadProject = (projectId: string, projectName: string): Promise<void> =>
  queue(async () => {
    await migrateLegacy(projectId, projectName).catch(console.error);
    await pushSection(projectId, (await readStore(projectId, projectName)).issues);
  });
