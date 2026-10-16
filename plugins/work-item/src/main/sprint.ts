// The Sprint block of a Project page shows its project's sprint. A key is loaded once for every block that shows it.

import { az } from "../lib/az";
import { ch } from "../lib/messages";
import { assignedQuery, blockItems, sprintQuery, type QueryRow } from "../lib/sections";
import { remoteFor, sprintPaths } from "./azure";

/** A block that opens gets the kept sprint, read again when it is older than this. */
const STALE_MS = 45_000;
const REFETCH_MS = 60_000;

export function serveSprint() {
  ch.serve(
    "sprint",
    async ({ path, mine }) => {
      const remote = await remoteFor(path);
      if (!remote) return { none: true };
      const { org, project } = remote;
      const paths = await sprintPaths({ org, project });
      if (!paths.length) return { org, project, sprints: [], items: [], total: 0 };
      const query = mine ? assignedQuery(project, paths) : sprintQuery(project, paths);
      const rows = await az<QueryRow[]>(org, ["boards", "query", "--project", project, "--wiql", query], { quiet: true, empty: [] });
      return { org, project, sprints: paths.map((p) => p.split("\\").at(-1) ?? p), items: blockItems(rows), total: rows.length };
    },
    { refetchMs: REFETCH_MS, staleMs: STALE_MS },
  );
}
