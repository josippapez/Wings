// The Issues tab: a board of to do, in progress, in review and done, with canceled behind a toggle, in rows by parent
// epic or feature unless that's turned off. Click a card to open the issue. Alt+Left and Alt+Right on a card move it
// between columns, and so do its buttons. List shows them as rows grouped by status instead. The view and the rows choice
// start as they were last left, or from the Default view setting, and the folded rows are kept with the tab. The search
// box filters what's shown.

import { useEffect, useState } from "react";
import { Loading, ProjectGate, useAction, useTabState } from "wings-plugin-ui";
import { BoardView } from "../ui/board-view";
import type { Issue, Op } from "../lib/domain";
import { useIssues } from "../ui/useIssues";
import { defaultView, lastLayout, startLayout, type Layout } from "../lib/view-setting";
import "./tasks.css";

export const page = { place: "tab", fill: true };

export default function BoardPage() {
  return (
    <ProjectGate loading="Loading issues" empty={{ description: "Open a project to see its issues." }}>
      {(project) => <ProjectBoard project={project} />}
    </ProjectGate>
  );
}

/** The rows folded shut, from what the tab saved. */
const foldedOf = (saved: unknown) => {
  const folded = (saved as { folded?: unknown }).folded;
  return { folded: Array.isArray(folded) ? folded.filter((k): k is string => typeof k === "string") : [] };
};

function ProjectBoard({ project }: { project: WingsProject }) {
  const { store, apply } = useIssues(project.id, project.name);
  const [showCanceled, setShowCanceled] = useState(false);
  // The layout and rows choice start as last picked in any Issues tab, else from the Default view setting.
  const [view, setView] = useState<Layout | null>(null);
  useEffect(() => void Promise.all([lastLayout.get(), defaultView.get()]).then(([last, v]) => setView(last ?? startLayout(v))), []);
  const pick = (next: Layout) => {
    setView(next);
    void lastLayout.set(next);
  };
  const [tab, setTab] = useTabState({ folded: [] as string[] }, foldedOf);
  const [query, setQuery] = useState("");
  const action = useAction();
  if (!view || !tab) return <Loading>Loading issues</Loading>;
  const send = (op: Op) => action.run(() => apply(op));
  const open = (issue: Issue, newTab: boolean) => void wings.openView("issue", { as: "tab", args: { key: issue.key, projectId: project.id }, newTab });
  const fold = (lane: string, isOpen: boolean) => setTab({ folded: isOpen ? tab.folded.filter((k) => k !== lane) : [...tab.folded, lane] });
  return (
    <BoardView
      project={project}
      issues={store?.issues ?? null}
      error={action.error}
      showCanceled={showCanceled}
      onShowCanceled={setShowCanceled}
      layout={view.layout}
      onLayout={(layout) => pick({ ...view, layout })}
      byParent={view.byParent}
      onByParent={(byParent) => pick({ ...view, byParent })}
      folded={tab.folded}
      onFold={fold}
      query={query}
      onQuery={setQuery}
      apply={apply}
      send={send}
      onOpen={open}
    />
  );
}
