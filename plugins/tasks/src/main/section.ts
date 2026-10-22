// A project's section in the sidebar: its open issues, the ones an agent works on now first, then by status or latest change,
// each epic's or feature's children under it. Wings skips rows identical to the last ones.

import { canParent, STATUS_LABEL, TYPE_LABEL, type Issue } from "../lib/domain";
import { sectionIssues } from "../lib/orderings";
import { childrenOf, progress } from "../lib/relations";
import { sectionOrder } from "../lib/section-setting";

const SECTION = "tasks";
/** The project section shows this many rows. */
const SECTION_ROWS = 50;

/** The sidebar row's status icon, which Wings draws in the badge's tone. */
const SECTION_ICON = { todo: "todo", in_progress: "in-progress", in_review: "in-review", done: "done", canceled: "canceled" } as const;

/** A muted line under a row for what isn't a plain task: its type, and an epic's or feature's progress. */
function detail(issues: Issue[], i: Issue): string {
  if (i.type === "task") return "";
  const { done, total } = progress(childrenOf(issues, i.key));
  return canParent(i.type) && total ? `${TYPE_LABEL[i.type]} · ${done} of ${total} done` : TYPE_LABEL[i.type];
}

/**
 * `issues` with each one's children right after it, and theirs after them, the rest in the order given. Wings indents a row under
 * its `parent`, and one that doesn't know `parent` still lists an epic over its children. An issue whose parent isn't in
 * `issues` stays where it was.
 */
function childrenAfterParents(issues: Issue[]): Issue[] {
  const keys = new Set(issues.map((i) => i.key));
  const below = new Map<string, Issue[]>();
  const top: Issue[] = [];
  for (const i of issues) {
    if (i.parent && keys.has(i.parent)) below.set(i.parent, [...(below.get(i.parent) ?? []), i]);
    else top.push(i);
  }
  const out: Issue[] = [];
  const placed = new Set<Issue>();
  const place = (i: Issue) => {
    if (placed.has(i)) return;
    placed.add(i);
    out.push(i);
    (below.get(i.key) ?? []).forEach(place);
  };
  // Whatever no top-level issue reaches hangs off a loop of parents, which still shows.
  [...top, ...issues].forEach(place);
  return out;
}

/** Each project's issues as last shown, so a pane change or the Sidebar order setting sorts them again without reading storage. */
const shown = new Map<string, Issue[]>();
/** The agent sessions running in a pane now. */
let running = new Set<string>();

const resort = () => Promise.all([...shown].map(([projectId, issues]) => pushSection(projectId, issues)));
sectionOrder.onChange(() => void resort());

/** Called with every pane on each change: sorts the sections again when an agent session starts or stops. */
export function onSectionPanes(panes: WingsPane[]): void {
  const now = new Set(panes.flatMap((p) => (p.session ? [p.session.sessionId] : [])));
  if (now.size === running.size && [...now].every((id) => running.has(id))) return;
  running = now;
  void resort();
}

/** Shows the open ones of `issues` under the project. A failure is logged, not thrown: it never fails the change that caused it. */
export async function pushSection(projectId: string, issues: Issue[]): Promise<void> {
  shown.set(projectId, issues);
  const order = await sectionOrder.get();
  // A newer push may have come in while the setting was read: show that one.
  const latest = shown.get(projectId)!;
  const open = sectionIssues(latest, (i) => i.sessions.some((s) => running.has(s.id)), order).slice(0, SECTION_ROWS);
  const listed = new Set(open.map((i) => i.key));
  const rows = childrenAfterParents(open).map((i) => ({
    id: i.key,
    label: `${i.key} ${i.title}`.slice(0, 120),
    ...(detail(latest, i) ? { detail: detail(latest, i) } : {}),
    ...(i.status === "todo" ? {} : { badge: { text: STATUS_LABEL[i.status], tone: i.status === "in_progress" ? ("info" as const) : ("warning" as const) } }),
    icon: SECTION_ICON[i.status],
    ...(i.parent && listed.has(i.parent) ? { parent: i.parent } : {}),
    open: { view: "issue", args: { key: i.key, projectId } },
  }));
  await wings.setProjectSection(projectId, SECTION, rows.length ? rows : null).catch(console.error);
}
