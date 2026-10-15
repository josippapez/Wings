// The badge on each pane whose branch names a work item, like `feature/1234-login`: its state, sprint and parent, with
// actions to open it and to move it to its next state.

import { cached, errorMessage } from "wings-plugin-sdk/async";
import { gitOk } from "wings-plugin-sdk/git";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { az, azCli, azRun, invoke } from "../lib/az";
import { workItemId } from "../lib/branch";
import { itemTabArgs } from "../lib/item";
import { ch } from "../lib/messages";
import { workItemUrl } from "../lib/urls";
import { badgeFor, dropClosed, nextStates, type Item, type Ref, type TypeState, type WorkItem } from "../lib/view";
import { remoteFor } from "./azure";

const BRANCH_POLL_MS = 15_000;
const ITEM_POLL_MS = 60_000;
const NOT_FOUND = /TF401232/;
const PROBLEM_TEXT = {
  title: "Azure DevOps",
  signedOut: "Sign in to Azure DevOps to see this branch's work item.",
  missing: "Work item needs the Azure CLI (az) to show this branch's work item.",
};

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

let panes: WingsPane[] = [];
/** paneId → { org, id, key } */
const refs = new Map<string, Ref>();
/** `${org} ${id}` → normalized work item, { missing: true } or { error } */
const items = new Map<string, Item>();
const inFlight = new Set<string>();
/** Items refreshed because you clicked something; background polls don't show a spinner. */
const manual = new Set<string>();
// `az boards work-item show` always prints the item, or fails.
const show = async (org: string, id: string | number) => (await az<AzWorkItem>(org, ["boards", "work-item", "show", "--id", String(id)]))!;

/** A parent's title, read once. */
const parentTitles = cached(async ({ org, id }: { org: string; id: number }) => (await show(org, id)).fields["System.Title"] ?? "", { key: JSON.stringify });
/** A work item type's states, in workflow order, read once. */
const typeStates = cached(
  async ({ org, project, type }: { org: string; project: string; type: string }) => (await az<{ value?: TypeState[] }>(org, invoke("wit", "workItemTypeStates", { project, type })))?.value ?? [],
  { key: JSON.stringify },
);

// ---------- finding the work item for a pane ----------

async function refreshRefs() {
  await Promise.all(
    panes.map(async (pane) => {
      const remote = await remoteFor(pane.cwd);
      const id = workItemId(remote ? ((await gitOk(pane.cwd, ["branch", "--show-current"])) ?? "") : "");
      if (!remote || !id) return void refs.delete(pane.paneId);
      const key = `${remote.org} ${id}`;
      refs.set(pane.paneId, { org: remote.org, id, key });
      if (!items.has(key)) void refreshItem(key);
    }),
  );
  dropClosed(refs, items, panes.map((p) => p.paneId));
  render();
}

// ---------- the work item ----------

async function refreshItem(key: string, { byUser = false } = {}) {
  const ref = [...refs.values()].find((r) => r.key === key);
  if (!ref) return;
  if (byUser) manual.add(key);
  if (inFlight.has(key)) return;
  inFlight.add(key);
  render();
  try {
    items.set(key, await loadItem(ref));
  } catch (error) {
    const message = errorMessage(error);
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
    typeStates.get({ org, project, type }).catch((): TypeState[] => []),
    f["System.Parent"] ? parentTitles.get({ org, id: f["System.Parent"] }).catch(() => "") : null,
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
  try {
    await azRun(ref.org, ["boards", "work-item", "update", "--id", ref.id, "--state", state], { quiet: true });
  } catch (error) {
    manual.delete(ref.key);
    render();
    throw error;
  }
  return refreshItem(ref.key, { byUser: true });
}

// ---------- badge ----------

function badgeOf(pane: WingsPane): WingsBadge | null {
  const ref = refs.get(pane.paneId);
  if (!ref) return null;
  const problem = azCli.problemBadge(ref.org, PROBLEM_TEXT);
  if (problem) return problem;
  const item = items.get(ref.key);
  if (item?.missing) return null;
  return { ...badgeFor(ref, item), loading: manual.has(ref.key) || (inFlight.has(ref.key) && !item) };
}

function render() {
  for (const pane of panes) void wings.setBadge(pane.paneId, badgeOf(pane));
}

export function showBadges() {
  wings.onPanes((list) => {
    panes = list;
    void refreshRefs();
  });
  // Both polls pause while Wings is in the background, and run once when you come back.
  pollWhileFocused(refreshRefs, BRANCH_POLL_MS);
  pollWhileFocused(() => new Set([...refs.values()].map((r) => r.key)).forEach((key) => void refreshItem(key)), ITEM_POLL_MS);

  azCli.onChange(render);
  // Try again, and signing in: what failed for lack of the CLI or a sign-in is read again.
  azCli.onRetry(() => {
    items.clear();
    parentTitles.clear();
    typeStates.clear();
    return refreshRefs();
  });
  // The item tab moved a work item to another state.
  ch.on("item-changed", ({ org, id }) => void refreshItem(`${org} ${id}`, { byUser: true }));

  wings.onAction(async ({ paneId, actionId }) => {
    if (actionId === "signin") return azCli.signIn();
    if (actionId === "retry") return azCli.retry();
    // Wings asks you, installs it in a tab and starts this plugin again once it's there.
    if (actionId === "install") return azCli.install();
    const ref = refs.get(paneId);
    if (!ref) return;
    if (actionId === "refresh") return refreshItem(ref.key, { byUser: true });
    const item = items.get(ref.key);
    if (actionId === "details") return wings.openView("item", { as: "tab", args: itemTabArgs(ref.id, ref.org) });
    if (actionId === "open" && item?.url) return wings.openUrl(item.url);
    if (actionId.startsWith("move:")) {
      const state = actionId.slice("move:".length);
      if (!item?.states || !nextStates(item).includes(state)) throw new Error(`${state} isn't a next state of this work item any more.`);
      return moveTo(ref, state);
    }
  });
}
