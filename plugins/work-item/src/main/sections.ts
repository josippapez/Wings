// The Work items section under each project in Wings' sidebar: the unfinished work items assigned to you in the
// sprint running now, that link to the project's repo or have a branch of it.

import { cached, errorMessage, singleFlight } from "wings-plugin-sdk/async";
import { gitOk } from "wings-plugin-sdk/git";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { az, invoke } from "../lib/az";
import { assignedQuery, isFinished, repoLinksOf, repoRows, SECTION_ID, stateCategories, type QueryRow, type RepoRef, type SectionRow } from "../lib/sections";
import { remoteFor, sprintPaths, type AzureRemote, type Project } from "./azure";

const POLL_MS = 120_000;
/** How long a project's rows are reused: a bit under the poll, so each poll reads again. */
const TEAM_ITEMS_TTL_MS = 100_000;
const CATEGORIES_TTL_MS = 3_600_000;
const FAILED_CATEGORIES_MS = 120_000;
/** The most unfinished items one team project's section looks at: one `workItems` read takes 200 ids. */
const ITEMS_CAP = 100;

/** Each work item type's state categories. A failed read is remembered briefly, so the next refresh tries again soon, and the state names decide meanwhile. */
const categories = cached(
  async ({ org, project }: Project) => {
    try {
      // `--query` keeps what az prints to the names and categories: the whole answer is over a megabyte.
      return stateCategories(await az<Parameters<typeof stateCategories>[0]>(org, invoke("wit", "workItemTypes", { project }, { jmes: "value[].{name:name,states:states[].{name:name,category:category}}" }), { quiet: true, empty: [] }));
    } catch (error) {
      console.warn("[work-item] state categories failed, using state names:", errorMessage(error));
      throw error;
    }
  },
  { ttlMs: CATEGORIES_TTL_MS, failTtlMs: FAILED_CATEGORIES_MS, key: JSON.stringify },
);

/** A repository's id, which links carry in place of its name. A failed read is asked again at the next refresh. */
const repoIds = cached(
  async ({ org, project, repo }: AzureRemote) => (await az<{ id?: string }>(org, invoke("git", "repositories", { project, repositoryId: repo }), { quiet: true, empty: {} })).id,
  { key: JSON.stringify },
);

interface TeamItems {
  rows: QueryRow[];
  /** work item id → ids of the repositories its links point at */
  links: Record<number, string[]>;
}

/** The unfinished items assigned to you in an Azure project's running sprints, with their repository links. Every repo of the project shares it. */
const teamItems = cached(
  async ({ org, project }: Project): Promise<TeamItems> => {
    const paths = await sprintPaths({ org, project });
    if (!paths.length) return { rows: [], links: {} };
    const found = await az<QueryRow[]>(org, ["boards", "query", "--project", project, "--wiql", assignedQuery(project, paths)], { quiet: true, empty: [] });
    const cats = await categories.get({ org, project }).catch(() => undefined);
    const rows = found.filter((r) => !isFinished(r.fields["System.WorkItemType"], r.fields["System.State"], cats)).slice(0, ITEMS_CAP);
    // One read for all the links, never one per item.
    const ids = rows.map((r) => r.id).join(",");
    const links = rows.length ? repoLinksOf(await az<Parameters<typeof repoLinksOf>[0]>(org, invoke("wit", "workItems", { project }, { query: [`ids=${ids}`, "$expand=Relations"] }), { quiet: true, empty: {} })) : {};
    return { rows, links };
  },
  { ttlMs: TEAM_ITEMS_TTL_MS, key: JSON.stringify },
);

/** The local and remote branches of a repo: cheap and local, so read at each refresh. */
async function branchesOf(cwd: string): Promise<string[]> {
  return ((await gitOk(cwd, ["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes"])) ?? "").split("\n").filter(Boolean);
}

async function rowsFor(remote: AzureRemote, cwd: string): Promise<SectionRow[]> {
  const { org, project } = remote;
  const team = await teamItems.get({ org, project });
  // Without the repository's id, only branches match.
  const ref: RepoRef = { id: await repoIds.get(remote).catch(() => undefined), branches: await branchesOf(cwd) };
  return repoRows(org, team.rows, team.links, ref);
}

/** One query per Azure project, whatever number of Wings projects point at it. Stale rows stay when a read fails. */
const refresh = singleFlight(async () => {
  // Wings in the background: the poll runs once when it comes back.
  if (!wings.focused) return;
  for (const { id, path } of await wings.projects()) {
    const remote = await remoteFor(path);
    let rows: SectionRow[] | null = null;
    if (remote) {
      try {
        rows = await rowsFor(remote, path);
      } catch (error) {
        console.warn("[work-item] work items section failed:", errorMessage(error));
        continue;
      }
    }
    await wings.setProjectSection(id, SECTION_ID, rows).catch((error) => console.warn("[work-item] setProjectSection failed:", errorMessage(error)));
  }
});

export function showSections() {
  wings.onProjects(() => void refresh());
  pollWhileFocused(() => void refresh(), POLL_MS);
}
