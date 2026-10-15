// Work item: shows the Azure Boards work item named in each pane's branch, like `feature/1234-login`,
// as a pane badge with its state, sprint and parent, and moves it to its next state.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { git, parseRemote, workItemId, type Remote } from "./lib/branch";
import { workItemUrl } from "./lib/urls";
import { assignedQuery, blockItems, isFinished, isFresh, currentIterations, repoLinksOf, repoRows, SECTION_ID, stateCategories, sprintQuery, type RepoRef, type StateCategories, type IterationNode, type QueryRow, type SectionRow, type SprintRequest, type SprintResponse } from "./lib/sections";
import { badgeFor, dropClosed, nextStates, type Item, type Ref, type TypeState, type WorkItem } from "./lib/view";

const BRANCH_POLL_MS = 15_000;
const ITEM_POLL_MS = 60_000;
const SIGN_IN_TIMEOUT_MS = 300_000;
const NOT_FOUND = /TF401232/;

/** The fields of `az boards work-item show` this plugin reads. */
interface AzWorkItem {
  id: number;
  fields: {
    "System.Title": string;
    "System.TeamProject": string;
    "System.WorkItemType": string;
    "System.State": string;
    "System.IterationPath"?: string;
    "System.Parent"?: number;
    "System.AssignedTo"?: { displayName?: string };
  };
}

interface Problem {
  login?: boolean;
  missing?: boolean;
  message: string;
}

let panes: WingsPane[] = [];
/** cwd → { kind: "azure", org } | { kind: "none" } */
const remotes = new Map<string, Remote>();
/** paneId → { org, id, key } */
const refs = new Map<string, Ref>();
/** `${org} ${id}` → normalized work item, { missing: true } or { error } */
const items = new Map<string, Item>();
const inFlight = new Set<string>();
/** Items refreshed because you clicked something; background polls don't show a spinner. */
const manual = new Set<string>();
/** `${org} ${id}` → Promise of the parent's title, fetched once. */
const parents = new Map<string, Promise<string>>();
/** `${org} ${project} ${type}` → Promise of the type's states, in workflow order. */
const typeStates = new Map<string, Promise<TypeState[]>>();
/** Azure org → { login, missing, message }, when `az` can't be used there at all: not installed, or signed out. Other errors are one item's, and show on its badge. */
const problems = new Map<string, Problem>();
let signingIn = false;

wings.onPanes((list) => {
  panes = list;
  void refreshRefs();
});
// Both polls pause while Wings is in the background, and run once when you come back.
pollWhileFocused(refreshRefs, BRANCH_POLL_MS);
pollWhileFocused(() => [...new Set([...refs.values()].map((r) => r.key))].forEach((key) => refreshItem(key)), ITEM_POLL_MS);

// ---------- finding the work item for a pane ----------

async function remoteFor(cwd: string) {
  if (!remotes.has(cwd)) {
    try {
      remotes.set(cwd, parseRemote(await git(cwd, ["remote", "get-url", "origin"])));
    } catch {
      remotes.set(cwd, { kind: "none" });
    }
  }
  return remotes.get(cwd)!;
}

async function refreshRefs() {
  await Promise.all(
    panes.map(async (pane) => {
      const remote = await remoteFor(pane.cwd);
      const branch = remote.kind === "azure" ? await git(pane.cwd, ["branch", "--show-current"]).catch(() => "") : "";
      const id = workItemId(branch);
      if (!id) return void refs.delete(pane.paneId);
      // A branch is only read for an Azure remote, so a work item id means `org` is set.
      const key = `${remote.org} ${id}`;
      refs.set(pane.paneId, { org: remote.org!, id, key });
      if (!items.has(key)) void refreshItem(key);
    }),
  );
  dropClosed(refs, items, panes.map((p) => p.paneId));
  render();
}

// ---------- the work item ----------

async function az(org: string, args: string[]): Promise<unknown> {
  let out;
  try {
    out = await wings.exec("az", [...args, "--organization", org, "-o", "json"]);
  } catch (error) {
    // How `wings.exec` fails for a program that isn't installed.
    if (/is not installed or not on PATH/.test(String((error as { message?: unknown }).message ?? error))) {
      problems.set(org, { missing: true, message: "Work item needs the Azure CLI (az) to show this branch's work item." });
    }
    throw error;
  }
  if (out.code === 0) {
    problems.delete(org);
    return JSON.parse(out.stdout);
  }
  const message = out.stderr.trim().split("\n").at(-1) || "az failed";
  if (/login/i.test(out.stderr)) problems.set(org, { login: true, message: "Sign in to Azure DevOps to see this branch's work item." });
  throw new Error(message);
}

const show = (org: string, id: string | number) => az(org, ["boards", "work-item", "show", "--id", String(id)]) as Promise<AzWorkItem>;

function statesOf(org: string, project: string, type: string) {
  const key = `${org} ${project} ${type}`;
  if (!typeStates.has(key)) {
    const load = az(org, [
      "devops", "invoke", "--area", "wit", "--resource", "workItemTypeStates",
      "--route-parameters", `project=${project}`, `type=${type}`, "--api-version", "7.1",
    ]).then((r) => (r as { value?: TypeState[] }).value ?? []);
    typeStates.set(key, load);
    load.catch(() => typeStates.delete(key));
  }
  return typeStates.get(key)!;
}

function parentTitle(org: string, id: number) {
  const key = `${org} ${id}`;
  if (!parents.has(key)) {
    const load = show(org, id).then((p) => p.fields["System.Title"] ?? "");
    parents.set(key, load);
    load.catch(() => parents.delete(key));
  }
  return parents.get(key)!;
}

function refOf(key: string) {
  return [...refs.values()].find((r) => r.key === key);
}

async function refreshItem(key: string, { byUser = false }: { byUser?: boolean } = {}) {
  const ref = refOf(key);
  if (!ref) return;
  if (byUser) manual.add(key);
  if (inFlight.has(key)) return;
  inFlight.add(key);
  render();
  try {
    items.set(key, await loadItem(ref));
  } catch (error) {
    const message = String((error as { message?: unknown }).message ?? error);
    items.set(key, NOT_FOUND.test(message) ? { missing: true } : { error: message || "az failed" });
  } finally {
    inFlight.delete(key);
    manual.delete(key);
    render();
  }
}

async function loadItem({ org, id }: Ref): Promise<WorkItem> {
  const wi = await show(org, id);
  const f = wi.fields;
  const project = f["System.TeamProject"];
  const type = f["System.WorkItemType"];
  const [states, parent] = await Promise.all([
    statesOf(org, project, type).catch((): TypeState[] => []),
    f["System.Parent"] ? parentTitle(org, f["System.Parent"]).catch(() => "") : null,
  ]);
  const iteration = f["System.IterationPath"] ?? "";
  return {
    id: wi.id,
    title: f["System.Title"],
    state: f["System.State"],
    type,
    category: states.find((s) => s.name === f["System.State"])?.category,
    states,
    sprint: iteration.includes("\\") ? iteration.split("\\").at(-1)! : "None",
    assigned: f["System.AssignedTo"]?.displayName ?? "Unassigned",
    parent: f["System.Parent"] ? `#${f["System.Parent"]} ${parent}`.trim() : null,
    url: workItemUrl(org, project, wi.id),
  };
}

/** The one write: a failure shows in the card, and doesn't count as a sign-in problem for the org. */
async function moveTo(ref: Ref, state: string) {
  manual.add(ref.key);
  render();
  const out = await wings.exec("az", ["boards", "work-item", "update", "--id", ref.id, "--state", state, "--organization", ref.org, "-o", "json"]);
  if (out.code !== 0) {
    manual.delete(ref.key);
    render();
    throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
  }
  return refreshItem(ref.key, { byUser: true });
}

// ---------- badge ----------

function problemBadge(problem: Problem): WingsBadge {
  if (problem.missing) {
    return {
      label: "Install Azure CLI",
      tone: "warning",
      title: "Azure DevOps",
      subtitle: problem.message,
      actions: [{ id: "install", label: "Install", primary: true }, { id: "retry", label: "Try again" }],
    };
  }
  return {
    label: signingIn ? "Signing in…" : "Sign in to Azure DevOps",
    tone: "warning",
    loading: signingIn,
    title: "Azure DevOps",
    subtitle: signingIn ? "Finish signing in in your browser." : problem.message,
    actions: [{ id: "signin", label: "Sign in", primary: true }, { id: "retry", label: "Try again" }],
  };
}

function render() {
  for (const pane of panes) {
    const ref = refs.get(pane.paneId);
    const item = ref && items.get(ref.key);
    const problem = ref && problems.get(ref.org);
    if (problem) {
      void wings.setBadge(pane.paneId, problemBadge(problem));
    } else if (ref && !item?.missing) {
      const loading = manual.has(ref.key) || (inFlight.has(ref.key) && !item);
      void wings.setBadge(pane.paneId, { ...badgeFor(ref, item), loading });
    } else {
      void wings.setBadge(pane.paneId, null);
    }
  }
}

async function signIn() {
  if (signingIn) return;
  signingIn = true;
  render();
  try {
    const out = await wings.exec("az", ["login", "--allow-no-subscriptions", "--output", "none"], { timeoutMs: SIGN_IN_TIMEOUT_MS });
    if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "Sign-in failed");
  } finally {
    signingIn = false;
  }
  return retry();
}

function retry() {
  problems.clear();
  items.clear();
  parents.clear();
  typeStates.clear();
  return refreshRefs();
}

wings.onAction(async ({ paneId, actionId }) => {
  if (actionId === "signin") return signIn();
  if (actionId === "retry") return retry();
  // Wings asks you, installs it in a tab and starts this plugin again once it's there.
  // (Wings ignores what an action resolves to, so installTool's boolean is dropped for the listener's type.)
  if (actionId === "install") return void (await wings.installTool("az"));
  const ref = refs.get(paneId);
  if (!ref) return;
  if (actionId === "refresh") return refreshItem(ref.key, { byUser: true });
  const item = items.get(ref.key);
  if (actionId === "details") return wings.openView("item", { as: "tab", args: { id: Number(ref.id), org: ref.org } });
  if (actionId === "open" && item?.url) return wings.openUrl(item.url);
  if (actionId.startsWith("move:")) {
    const state = actionId.slice("move:".length);
    if (!item?.states || !nextStates(item).includes(state)) throw new Error(`${state} isn't a next state of this work item any more.`);
    return moveTo(ref, state);
  }
});

/** `az` for the background check: a failure there is the check's, and doesn't turn the pane badges into a sign-in prompt. */
async function azQuiet(org: string, args: string[]): Promise<unknown> {
  const out = await wings.exec("az", [...args, "--organization", org, "-o", "json"]);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
  // `az boards query` prints nothing for an empty result.
  return out.stdout.trim() ? JSON.parse(out.stdout) : [];
}

// The item tab moved a work item to another state.
wings.onBroadcast((message) => {
  if ((message as { type?: string } | null)?.type === "item-changed") for (const key of new Set([...refs.values()].map((r) => r.key))) void refreshItem(key, { byUser: true });
});

// ---------- projects' Work items sections ----------

/** How long a project's rows, and its running sprints, are reused: a bit under the shortest poll interval. */
const SECTION_TTL_MS = 100_000;
const SECTION_POLL_MS = 120_000;
const SPRINT_TTL_MS = 3_600_000;
const FAILED_CATEGORIES_TTL_MS = 120_000;

/** `${org} ${project}` → the running sprints' paths, when they were read. */
const sprints = new Map<string, { at: number; paths: string[] }>();
/** projectId → the rows last sent to Wings, as JSON, so an unchanged section isn't sent again. */
const sent = new Map<string, string>();
let sectionsRunning = false;
let sectionsAgain = false;

/** The paths of the sprints running now in an Azure project, read at most once an hour. */
async function sprintPaths(org: string, project: string): Promise<string[]> {
  const key = `${org} ${project}`;
  let known = sprints.get(key);
  if (!known || Date.now() - known.at > SPRINT_TTL_MS) {
    const root = (await azQuiet(org, [
      "devops", "invoke", "--area", "wit", "--resource", "classificationNodes",
      "--route-parameters", `project=${project}`, "structureGroup=iterations", "--query-parameters", "$depth=4", "--api-version", "7.1",
    ])) as IterationNode | never[];
    known = { at: Date.now(), paths: currentIterations(Array.isArray(root) ? null : root, Date.now()) };
    sprints.set(key, known);
  }
  return known.paths;
}

/** `${org} ${project}` → each work item type's state categories, read at most once an hour. A failed read is `null`: names decide then. */
const categories = new Map<string, { at: number; value: StateCategories | null }>();
/** `${org} ${project} ${repo}` → the repository's id, which links carry in place of its name. */
const repoIds = new Map<string, string | undefined>();

async function stateCategoriesFor(org: string, project: string): Promise<StateCategories | null> {
  const key = `${org} ${project}`;
  const known = categories.get(key);
  if (known && Date.now() - known.at < SPRINT_TTL_MS) return known.value;
  let value: StateCategories | null = null;
  try {
    // `--query` keeps what az prints to the names and categories: the whole answer is over a megabyte.
    value = stateCategories((await azQuiet(org, [
      "devops", "invoke", "--area", "wit", "--resource", "workItemTypes", "--route-parameters", `project=${project}`, "--api-version", "7.1",
      "--query", "value[].{name:name,states:states[].{name:name,category:category}}",
    ])) as never);
  } catch (error) {
    console.warn("[work-item] state categories failed, using state names:", error instanceof Error ? error.message : error);
  }
  // A failed read is only remembered briefly, so the next refresh tries again soon.
  categories.set(key, { at: value ? Date.now() : Date.now() - SPRINT_TTL_MS + FAILED_CATEGORIES_TTL_MS, value });
  return value;
}

async function repoIdFor(org: string, project: string, repo: string): Promise<string | undefined> {
  const key = `${org} ${project} ${repo}`;
  if (!repoIds.has(key)) {
    try {
      const found = (await azQuiet(org, ["devops", "invoke", "--area", "git", "--resource", "repositories", "--route-parameters", `project=${project}`, `repositoryId=${repo}`, "--api-version", "7.1"])) as { id?: string };
      repoIds.set(key, found.id);
    } catch {
      // Not remembered: the next refresh asks again, and until then only branches match.
      return undefined;
    }
  }
  return repoIds.get(key);
}

/** The most unfinished items one team project's section looks at: one `workItems` read takes 200 ids. */
const ITEMS_CAP = 100;

interface TeamItems {
  rows: QueryRow[];
  /** work item id → ids of the repositories its links point at */
  links: Record<number, string[]>;
}

/** `${org} ${project}` → the unfinished items assigned to you, with their repository links, when they were read. Every repo of the team project shares it. */
const teamItems = new Map<string, { at: number; value: TeamItems }>();

async function teamItemsFor(org: string, project: string): Promise<TeamItems> {
  const key = `${org} ${project}`;
  const cached = teamItems.get(key);
  if (cached && Date.now() - cached.at < SECTION_TTL_MS) return cached.value;
  const paths = await sprintPaths(org, project);
  let value: TeamItems = { rows: [], links: {} };
  if (paths.length) {
    const found = await azQuiet(org, ["boards", "query", "--project", project, "--wiql", assignedQuery(project, paths)]);
    const cats = await stateCategoriesFor(org, project);
    const rows = (Array.isArray(found) ? (found as QueryRow[]) : []).filter((r) => !isFinished(r.fields["System.WorkItemType"], r.fields["System.State"], cats ?? undefined)).slice(0, ITEMS_CAP);
    // One read for all the links, never one per item.
    const links = rows.length
      ? repoLinksOf((await azQuiet(org, ["devops", "invoke", "--area", "wit", "--resource", "workItems", "--route-parameters", `project=${project}`, "--query-parameters", `ids=${rows.map((r) => r.id).join(",")}`, "$expand=Relations", "--api-version", "7.1"])) as never)
      : {};
    value = { rows, links };
  }
  teamItems.set(key, { at: Date.now(), value });
  return value;
}

/** The local and remote branches of a repo: cheap and local, so read at each refresh. */
async function branchesOf(cwd: string): Promise<string[]> {
  try {
    return (await git(cwd, ["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes"])).split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

async function sectionRowsFor(remote: { org: string; project: string; repo: string }, cwd: string): Promise<SectionRow[]> {
  const { org, project, repo } = remote;
  const team = await teamItemsFor(org, project);
  const ref: RepoRef = { id: await repoIdFor(org, project, repo), branches: await branchesOf(cwd) };
  return repoRows(org, project, team.rows, team.links, ref);
}

/** One query per Azure project, whatever number of Wings projects point at it. Stale rows stay when a read fails. */
async function refreshSections() {
  // Wings in the background: the poll runs once when it comes back.
  if (!wings.focused) return;
  if (sectionsRunning) {
    sectionsAgain = true;
    return;
  }
  sectionsRunning = true;
  try {
    for (const project of await wings.projects()) {
      const remote = await remoteFor(project.path);
      let rows: SectionRow[] | null = null;
      if (remote.kind === "azure") {
        try {
          rows = await sectionRowsFor(remote, project.path);
        } catch (error) {
          console.warn("[work-item] work items section failed:", error instanceof Error ? error.message : error);
          continue;
        }
      }
      const json = JSON.stringify(rows);
      if (sent.get(project.id) === json) continue;
      try {
        await wings.setProjectSection(project.id, SECTION_ID, rows);
        sent.set(project.id, json);
      } catch (error) {
        console.warn("[work-item] setProjectSection failed:", error instanceof Error ? error.message : error);
      }
    }
  } finally {
    sectionsRunning = false;
    if (sectionsAgain) {
      sectionsAgain = false;
      void refreshSections();
    }
  }
}

wings.onProjects(() => void refreshSections());
pollWhileFocused(() => void refreshSections(), SECTION_POLL_MS);

// ---------- the Sprint block ----------

/** A little under the block's 60 s poll: a poll reads fresh, and blocks that ask together share one query. Older than this is never served. */
const BLOCK_TTL_MS = 55_000;

type SprintAnswer = Omit<SprintResponse, "type" | "rid">;

/** `${org} ${project} ${mine}` → the answer, when it was read. A read in flight is shared, so two blocks make one query. */
const blocks = new Map<string, { at: number; answer: Promise<SprintAnswer> }>();

function sprintFor(org: string, project: string, mine: boolean): Promise<SprintAnswer> {
  const key = `${org} ${project} ${mine}`;
  const hit = blocks.get(key);
  if (hit && isFresh(hit.at, Date.now(), BLOCK_TTL_MS)) return hit.answer;
  const answer = (async (): Promise<SprintAnswer> => {
    const paths = await sprintPaths(org, project);
    if (!paths.length) return { org, project, sprints: [], items: [], total: 0 };
    const query = mine ? assignedQuery(project, paths) : sprintQuery(project, paths);
    const found = await azQuiet(org, ["boards", "query", "--project", project, "--wiql", query]);
    const rows = Array.isArray(found) ? (found as QueryRow[]) : [];
    return { org, project, sprints: paths.map((p) => p.split("\\").at(-1) ?? p), items: blockItems(rows), total: rows.length };
  })();
  blocks.set(key, { at: Date.now(), answer });
  // A failed read isn't kept, so the next ask tries again.
  answer.catch(() => blocks.get(key)?.answer === answer && blocks.delete(key));
  return answer;
}

/** A Sprint block on a Project page asks for its project's sprint. It only asks while Wings is in front and the block is on screen. */
wings.onBroadcast(async (message) => {
  const request = message as SprintRequest | null;
  if (request?.type !== "sprint-request" || typeof request.path !== "string") return;
  let answer: SprintAnswer;
  try {
    const remote = await remoteFor(request.path);
    answer = remote.kind === "azure" ? await sprintFor(remote.org, remote.project, request.mine !== false) : { none: true };
  } catch (error) {
    answer = { error: String((error as Error).message ?? error).slice(0, 300) };
  }
  await wings.broadcast({ type: "sprint-response", rid: request.rid, ...answer } satisfies SprintResponse);
});
