// The Issues tab: a board of to do, in progress, in review and done, with canceled behind a toggle. Click a card to open
// the issue. Alt+Left and Alt+Right on a card move it between columns, and so do its buttons.

import { useEffect, useState } from "react";
import { Empty, Loading, showTab } from "wings-plugin-ui";
import { BoardView } from "./lib/board-view";
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
  const [error, setError] = useState<string | null>(null);
  const send = (op: Op) => apply(op).then(() => setError(null), (e) => setError(e instanceof Error ? e.message : String(e)));
  const open = (issue: Issue, newTab: boolean) => void wings.openView("issue", { as: "tab", args: { key: issue.key, projectId: project.id }, newTab });
  return <BoardView project={project} issues={store?.issues ?? null} error={error} showCanceled={showCanceled} onShowCanceled={setShowCanceled} apply={apply} send={send} onOpen={open} />;
}

showTab(<BoardPage />, { fill: true });
