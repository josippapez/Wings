// The Issues tab: a board of to do, in progress, in review and done, with canceled behind a toggle, in rows by parent
// epic or feature unless that's turned off. Click a card to open the issue. Alt+Left and Alt+Right on a card move it
// between columns, and so do its buttons. List shows them as rows grouped by status instead. The view and the rows choice
// start from the Default view setting, and the folded rows are kept with the tab. The search box filters what's shown.

import { useEffect, useState } from "react";
import { Empty, Loading } from "wings-plugin-ui";
import { BoardView, type Layout } from "../ui/board-view";
import type { Issue, Op } from "../lib/domain";
import { useIssues } from "../lib/useIssues";
import { DEFAULT_VIEW_KEY, defaultViewOf, startLayout } from "../lib/view-setting";
import "./tasks.css";

export const page = { place: "tab", fill: true };

export default function BoardPage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  if (project === undefined) return <Loading>Loading issues</Loading>;
  if (!project) return <Empty title="No project" description="Open a project to see its issues." />;
  return <ProjectBoard key={project.id} project={project} />;
}

function ProjectBoard({ project }: { project: WingsProject }) {
  const { store, apply } = useIssues(project.id, project.name);
  const [showCanceled, setShowCanceled] = useState(false);
  const [view, setView] = useBoardView();
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const send = (op: Op) => apply(op).then(() => setError(null), (e) => setError(e instanceof Error ? e.message : String(e)));
  const open = (issue: Issue, newTab: boolean) => void wings.openView("issue", { as: "tab", args: { key: issue.key, projectId: project.id }, newTab });
  if (!view) return <Loading>Loading issues</Loading>;
  const fold = (lane: string, isOpen: boolean) => setView({ ...view, folded: isOpen ? view.folded.filter((k) => k !== lane) : [...view.folded, lane] });
  return (
    <BoardView
      project={project}
      issues={view && store ? store.issues : null}
      error={error}
      showCanceled={showCanceled}
      onShowCanceled={setShowCanceled}
      layout={view.layout}
      onLayout={(layout) => setView({ ...view, layout })}
      byParent={view.byParent}
      onByParent={(byParent) => setView({ ...view, byParent })}
      folded={view.folded}
      onFold={fold}
      query={query}
      onQuery={setQuery}
      apply={apply}
      send={send}
      onOpen={open}
    />
  );
}

interface View {
  layout: Layout;
  byParent: boolean;
  folded: string[];
}

/**
 * The layout and rows choice start from the Default view setting each time the tab opens, and changing them lasts until it
 * closes. The folded rows are kept with the tab. `null` until both are read.
 */
function useBoardView(): [View | null, (v: View) => void] {
  const [view, setView] = useState<View | null>(null);
  useEffect(() => {
    void Promise.all([wings.tabState<{ folded?: unknown }>().catch(() => null), wings.storage.get(DEFAULT_VIEW_KEY).catch(() => null)]).then(([saved, setting]) =>
      setView({ ...startLayout(defaultViewOf(setting)), folded: Array.isArray(saved?.folded) ? saved.folded.filter((k): k is string => typeof k === "string") : [] }),
    );
  }, []);
  const save = (v: View) => {
    setView(v);
    void wings.setTabState({ folded: v.folded }).catch(() => {});
  };
  return [view, save];
}
