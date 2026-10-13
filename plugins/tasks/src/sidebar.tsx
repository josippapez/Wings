// The Tasks sidebar: the short list for the project on screen, to add a task and tick one off.

import { useEffect, useState } from "react";
import { Empty, Loading, showSidebar } from "wings-plugin-ui";
import { SidebarView } from "./lib/sidebar-view";
import { useTasks } from "./lib/useTasks";
import "./tasks.css";

function Sidebar() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  if (project === undefined) return <Loading>Loading tasks</Loading>;
  if (!project) return <Empty title="No project on screen" description="Open a project to see its tasks." />;
  return <ProjectTasks key={project.id} project={project} />;
}

function ProjectTasks({ project }: { project: WingsProject }) {
  const { tasks, apply } = useTasks(project.id);
  const board = (taskId?: string) => void wings.openView("board", { as: "tab", args: taskId ? { taskId } : undefined });
  return <SidebarView project={project} tasks={tasks} apply={apply} board={board} />;
}

showSidebar(<Sidebar />);
