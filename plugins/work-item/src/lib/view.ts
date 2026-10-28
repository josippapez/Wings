/** A pane's work item: its org, its id from the branch, and `${org} ${id}`. */
export interface Ref {
  org: string;
  id: string;
  key: string;
}

/** One state of a work item type, from `workItemTypeStates`. */
export interface TypeState {
  name: string;
  category: string;
}

export interface WorkItem {
  id: number;
  title: string;
  state: string;
  type: string;
  category: string | undefined;
  states: TypeState[];
  sprint: string;
  assigned: string;
  parent: string | null;
  url: string;
  missing?: undefined;
  error?: undefined;
}

/** A normalized work item, `{ missing: true }` or `{ error }`. */
export type Item =
  | WorkItem
  | { missing: true; error?: undefined; title?: undefined; url?: undefined; states?: undefined }
  | { error: string; missing?: undefined; title?: undefined; url?: undefined; states?: undefined };

/** Forgets the panes that are gone: their refs, and the items no ref points at, so a closed pane's work item isn't looked up again. */
export function dropClosed(refs: Map<string, Ref>, items: Map<string, Item>, open: Iterable<string>) {
  const panes = new Set(open);
  for (const paneId of refs.keys()) if (!panes.has(paneId)) refs.delete(paneId);
  const keys = new Set([...refs.values()].map((r) => r.key));
  for (const key of items.keys()) if (!keys.has(key)) items.delete(key);
}

/** Process categories: Proposed, InProgress, Resolved, Completed, Removed. Review and QA states wait on someone else. */
function toneOf(item: WorkItem): Tone {
  if (item.category === "InProgress") return /review|qa|test/i.test(item.state) ? "warning" : "info";
  return ({ Resolved: "merged", Completed: "success" } as Record<string, Tone>)[item.category as string] ?? "neutral";
}

/** The next two states in the type's workflow order, skipping Removed. */
export function nextStates(item: Pick<WorkItem, "state" | "states">) {
  const forward = item.states.filter((s) => s.category !== "Removed");
  const at = forward.findIndex((s) => s.name === item.state);
  if (at < 0) return [];
  return forward.slice(at + 1, at + 3).map((s) => s.name).filter((name) => name.length <= 32);
}

/** Every other state of the type, in workflow order, as Azure DevOps' own state menu offers them. */
export function otherStates(item: Pick<WorkItem, "state" | "states">) {
  return item.states.map((s) => s.name).filter((name) => name !== item.state);
}

export function badgeFor(ref: Ref, item: Exclude<Item, { missing: true }> | undefined): WingsBadge {
  const refresh = { id: "refresh", label: "Refresh" };
  const label = `#${ref.id}`;
  // Until the item loads there's no link to open.
  if (!item) return { label, tone: "neutral", title: "Loading work item…", subtitle: `Work item #${ref.id}`, actions: [refresh] };
  if (item.error !== undefined) return { label, tone: "neutral", title: `Work item #${ref.id}`, rows: [{ label: "Status", value: item.error, tone: "danger" }], actions: [{ ...refresh, primary: true }] };
  const actions = [{ id: "details", label: "Open details", primary: true }, { id: "open", label: "Open in Azure DevOps" }, refresh];
  const tone = toneOf(item);
  return {
    label: `#${ref.id} ${item.state}`,
    tone,
    title: item.title,
    subtitle: `${item.type} #${ref.id}`,
    rows: [
      { label: "Type", value: item.type },
      { label: "State", value: item.state, tone },
      { label: "Sprint", value: item.sprint },
      { label: "Assigned", value: item.assigned },
      ...(item.parent ? [{ label: "Parent", value: item.parent }] : []),
    ],
    actions: [...actions, ...nextStates(item).map((s) => ({ id: `move:${s}`, label: `Move to ${s}` }))],
  };
}
