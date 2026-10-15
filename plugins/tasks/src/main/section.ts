// A project's section in the sidebar: its open issues, in progress first. Wings skips rows identical to the last ones.

import { canParent, STATUS_LABEL, TYPE_LABEL, type Issue } from "../lib/domain";
import { openIssues } from "../lib/orderings";
import { childrenOf, progress } from "../lib/relations";

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

/** Shows the open ones of `issues` under the project. A failure is logged, not thrown: it never fails the change that caused it. */
export function pushSection(projectId: string, issues: Issue[]): Promise<void> {
  const rows = openIssues(issues)
    .slice(0, SECTION_ROWS)
    .map((i) => ({
      id: i.key,
      label: `${i.key} ${i.title}`.slice(0, 120),
      ...(detail(issues, i) ? { detail: detail(issues, i) } : {}),
      ...(i.status === "todo" ? {} : { badge: { text: STATUS_LABEL[i.status], tone: i.status === "in_progress" ? ("info" as const) : ("warning" as const) } }),
      icon: SECTION_ICON[i.status],
      open: { view: "issue", args: { key: i.key, projectId } },
    }));
  return wings.setProjectSection(projectId, SECTION, rows.length ? rows : null).catch(console.error);
}
