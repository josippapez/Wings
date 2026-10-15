// The Issues tab: a board of to do, in progress, in review and done, with canceled behind a toggle, in rows by parent
// epic or feature unless that's turned off. Click a card to open the issue. Alt+Left and Alt+Right on a card move it
// between columns, and so do its buttons. List shows them as rows grouped by status instead. The view, the rows choice
// and the folded rows are kept with the tab.

import { useEffect, useState } from "react";
import { Empty, Loading, showTab } from "wings-plugin-ui";
import { BoardView, type Layout } from "./lib/board-view";
import type { Issue, Op } from "./lib/issues";
import { useIssues } from "./lib/useIssues";
import "./tasks.css";

function BoardPage() {
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
  const [error, setError] = useState<string | null>(null);
  const send = (op: Op) => apply(op).then(() => setError(null), (e) => setError(e instanceof Error ? e.message : String(e)));
  const open = (issue: Issue, newTab: boolean) => void wings.openView("issue", { as: "tab", args: { key: issue.key, projectId: project.id }, newTab });
  const fold = (lane: string, isOpen: boolean) => setView({ ...view, folded: isOpen ? view.folded.filter((k) => k !== lane) : [...view.folded, lane] });
  return (
    <BoardView
      project={project}
      issues={store?.issues ?? null}
      error={error}
      showCanceled={showCanceled}
      onShowCanceled={setShowCanceled}
      layout={view.layout}
      onLayout={(layout) => setView({ ...view, layout })}
      byParent={view.byParent}
      onByParent={(byParent) => setView({ ...view, byParent })}
      folded={view.folded}
      onFold={fold}
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

/** Board or list, rows by parent (on until turned off) and the folded rows, kept with the tab so they are there when it opens again. */
function useBoardView(): [View, (v: View) => void] {
  const [view, setView] = useState<View>({ layout: "board", byParent: true, folded: [] });
  useEffect(() => {
    void wings
      .tabState<Partial<View>>()
      .then((saved) => setView({ layout: saved?.layout === "list" ? "list" : "board", byParent: saved?.byParent !== false, folded: Array.isArray(saved?.folded) ? saved.folded.filter((k) => typeof k === "string") : [] }))
      .catch(() => {});
  }, []);
  const save = (v: View) => {
    setView(v);
    void wings.setTabState(v).catch(() => {});
  };
  return [view, save];
}

showTab(<BoardPage />, { fill: true });
