// The Tasks tab: a board of to do, doing and done. Click a card to edit it. Alt+Left and Alt+Right on a card move it
// between columns, and so do its buttons.

import { useEffect, useState } from "react";
import { Empty, Loading, showTab } from "wings-plugin-ui";
import { BoardView } from "./lib/board-view";
import type { Op } from "./lib/tasks";
import { useTasks } from "./lib/useTasks";
import "./tasks.css";

function BoardPage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  if (project === undefined) return <Loading>Loading tasks</Loading>;
  if (!project) return <Empty title="No project" description="Open a project to see its tasks." />;
  return <ProjectBoard key={project.id} project={project} />;
}

function ProjectBoard({ project }: { project: WingsProject }) {
  const { tasks, apply } = useTasks(project.id);
  const [selected, setSelected] = useState<string | null>(() => wings.viewArgs<{ taskId?: string }>()?.taskId ?? null);
  const [error, setError] = useState<string | null>(null);
  const send = (op: Op) => apply(op).then(() => setError(null), (e) => setError(e instanceof Error ? e.message : String(e)));
  return <BoardView project={project} tasks={tasks} error={error} selected={selected} onSelect={setSelected} apply={apply} send={send} />;
}

showTab(<BoardPage />, { fill: true });
