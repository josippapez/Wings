// Work item: shows the Azure Boards work item named in each pane's branch, like `feature/1234-login`,
// as a pane badge with its state, sprint and parent, and moves it to its next state. It also polls Azure DevOps
// for updates to your work items and pull requests (see lib/watch.ts) and tells you about them.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { git, parseRemote, workItemId, type Remote } from "./lib/branch";
import { createWatcher, DEFAULT_SETTINGS, type Settings } from "./lib/watch";
import { workItemUrl, type StoredEvent } from "./lib/updates";
import { assignedQuery, currentIterations, SECTION_ID, sectionRows, type IterationNode, type QueryRow, type SectionRow } from "./lib/sections";
import { badgeFor, nextStates, type Item, type Ref, type TypeState, type WorkItem } from "./lib/view";

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

/** Folders whose Azure project the Updates watcher was told about. Only panes put a project there, not the sidebar's sections. */
const watched = new Set<string>();

async function remoteFor(cwd: string, { watch = false } = {}) {
  if (!remotes.has(cwd)) {
    try {
      remotes.set(cwd, parseRemote(await git(cwd, ["remote", "get-url", "origin"])));
    } catch {
      remotes.set(cwd, { kind: "none" });
    }
  }
  const remote = remotes.get(cwd)!;
  if (watch && remote.kind === "azure" && !watched.has(cwd)) {
    watched.add(cwd);
    void watcher.addProject(remote.org, remote.project);
  }
  return remote;
}

async function refreshRefs() {
  await Promise.all(
    panes.map(async (pane) => {
      const remote = await remoteFor(pane.cwd, { watch: true });
      const branch = remote.kind === "azure" ? await git(pane.cwd, ["branch", "--show-current"]).catch(() => "") : "";
      const id = workItemId(branch);
      if (!id) return void refs.delete(pane.paneId);
      // A branch is only read for an Azure remote, so a work item id means `org` is set.
      const key = `${remote.org} ${id}`;
      refs.set(pane.paneId, { org: remote.org!, id, key });
      if (!items.has(key)) void refreshItem(key);
    }),
  );
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
  void watcher.addProject(org, project);
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

// ---------- updates ----------

/** `az` for the background check: a failure there is the check's, and doesn't turn the pane badges into a sign-in prompt. */
async function azQuiet(org: string, args: string[]): Promise<unknown> {
  const out = await wings.exec("az", [...args, "--organization", org, "-o", "json"]);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
  // `az boards query` prints nothing for an empty result.
  return out.stdout.trim() ? JSON.parse(out.stdout) : [];
}

const watcher = createWatcher({
  az: azQuiet,
  get: (key) => wings.storage.get(key),
  set: (key, value) => wings.storage.set(key, value),
  notify: (note) => wings.notify(note),
  now: Date.now,
});

let settings: Settings = DEFAULT_SETTINGS;
let timer: ReturnType<typeof setInterval> | undefined;
let checking = false;

async function checkUpdates() {
  // A check that outlasts the interval makes the next tick wait, rather than pile up az processes.
  if (checking || !settings.enabled) return;
  checking = true;
  try {
    if ((await watcher.cycle(await watcher.targets(), settings.intervalMin * 60_000)).length) wings.broadcast({ type: "updates" });
    await showUnread();
  } finally {
    checking = false;
  }
}

async function showUnread() {
  const unread = (await watcher.stored()).filter((e: StoredEvent) => !e.read).length;
  await wings.setSidebarLabel("updates", unread ? `${unread} new` : null);
}

async function loadSettings() {
  const saved = (await wings.storage.get("settings")) as Partial<Settings> | null;
  settings = { ...DEFAULT_SETTINGS, ...saved };
  clearInterval(timer);
  // Not tied to focus: it keeps running while Wings is in the background, which is when you want to hear about it.
  timer = settings.enabled ? setInterval(() => void checkUpdates(), settings.intervalMin * 60_000) : undefined;
}

wings.onBroadcast((message) => {
  const m = message as { type?: string; settingsChanged?: boolean; rid?: unknown; op?: string; key?: unknown } | null;
  if (m?.settingsChanged) void loadSettings();
  // The Updates pages don't write storage: main is its only writer, so their changes and new events can't overwrite each other.
  if (m?.type === "events-op" && typeof m.rid === "string") {
    const done = m.op === "clear" ? watcher.clear() : m.op === "read" ? watcher.markRead(typeof m.key === "string" ? m.key : undefined) : null;
    void done?.then(() => showUnread()).then(() => wings.broadcast({ type: "updates", rid: m.rid }));
  }
  // The item tab moved a work item to another state.
  if (m?.type === "item-changed") for (const key of new Set([...refs.values()].map((r) => r.key))) void refreshItem(key, { byUser: true });
});
void loadSettings().then(() => setTimeout(() => void checkUpdates(), 15_000));

// ---------- projects' Work items sections ----------

/** How long a project's rows, and its running sprints, are reused: a bit under the shortest poll interval. */
const SECTION_TTL_MS = 100_000;
const SECTION_POLL_MS = 120_000;
const SPRINT_TTL_MS = 3_600_000;

/** `${org} ${project}` → the rows, when they were read. */
const sections = new Map<string, { at: number; rows: SectionRow[] }>();
/** `${org} ${project}` → the running sprints' paths, when they were read. */
const sprints = new Map<string, { at: number; paths: string[] }>();
/** projectId → the rows last sent to Wings, as JSON, so an unchanged section isn't sent again. */
const sent = new Map<string, string>();
let sectionsRunning = false;
let sectionsAgain = false;

async function sectionRowsFor(org: string, project: string): Promise<SectionRow[]> {
  const key = `${org} ${project}`;
  const cached = sections.get(key);
  if (cached && Date.now() - cached.at < SECTION_TTL_MS) return cached.rows;
  let known = sprints.get(key);
  if (!known || Date.now() - known.at > SPRINT_TTL_MS) {
    const root = (await azQuiet(org, [
      "devops", "invoke", "--area", "wit", "--resource", "classificationNodes",
      "--route-parameters", `project=${project}`, "structureGroup=iterations", "--query-parameters", "$depth=4", "--api-version", "7.1",
    ])) as IterationNode | never[];
    known = { at: Date.now(), paths: currentIterations(Array.isArray(root) ? null : root, Date.now()) };
    sprints.set(key, known);
  }
  let rows: SectionRow[] = [];
  if (known.paths.length) {
    const found = await azQuiet(org, ["boards", "query", "--project", project, "--wiql", assignedQuery(project, known.paths)]);
    rows = sectionRows(org, project, Array.isArray(found) ? (found as QueryRow[]) : []);
  }
  sections.set(key, { at: Date.now(), rows });
  return rows;
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
          rows = await sectionRowsFor(remote.org, remote.project);
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
