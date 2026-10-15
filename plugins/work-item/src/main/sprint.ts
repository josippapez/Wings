// The Sprint block of a Project page asks for its project's sprint. A read is shared by every block that asks.

import { cached } from "wings-plugin-sdk/async";
import { az } from "../lib/az";
import { ch, type Sprint } from "../lib/messages";
import { assignedQuery, blockItems, sprintQuery, type QueryRow } from "../lib/sections";
import { remoteFor, sprintPaths, type Project } from "./azure";

/** Counted from when a read finishes, so it is under the block's 60 s poll by more than a slow read takes: a poll reads fresh, and blocks that ask together share one query. */
const TTL_MS = 45_000;

const sprints = cached(
  async ({ org, project, mine }: Project & { mine: boolean }): Promise<Sprint> => {
    const paths = await sprintPaths({ org, project });
    if (!paths.length) return { org, project, sprints: [], items: [], total: 0 };
    const query = mine ? assignedQuery(project, paths) : sprintQuery(project, paths);
    const rows = (await az<QueryRow[]>(org, ["boards", "query", "--project", project, "--wiql", query], { quiet: true })) ?? [];
    return { org, project, sprints: paths.map((p) => p.split("\\").at(-1) ?? p), items: blockItems(rows), total: rows.length };
  },
  { ttlMs: TTL_MS, key: JSON.stringify },
);

export function answerSprint() {
  ch.answer("sprint", async ({ path, mine }) => {
    const remote = await remoteFor(path);
    return remote ? sprints.get({ org: remote.org, project: remote.project, mine }) : { none: true };
  });
}
