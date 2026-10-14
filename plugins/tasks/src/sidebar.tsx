// The Issues sidebar: the open issues of the project on screen, to add one and tick one off.

import { useEffect, useState } from "react";
import { Empty, Loading, showSidebar } from "wings-plugin-ui";
import type { Issue } from "./lib/issues";
import { SidebarView } from "./lib/sidebar-view";
import { useIssues } from "./lib/useIssues";
import "./tasks.css";

function Sidebar() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  if (project === undefined) return <Loading>Loading issues</Loading>;
  if (!project) return <Empty title="No project on screen" description="Open a project to see its issues." />;
  return <ProjectIssues key={project.id} project={project} />;
}

function ProjectIssues({ project }: { project: WingsProject }) {
  const { store, apply } = useIssues(project.id, project.name);
  const openBoard = () => void wings.openView("board", { as: "tab" });
  const openIssue = (issue: Issue) => void wings.openView("issue", { as: "tab", args: { key: issue.key, projectId: project.id } });
  return <SidebarView project={project} issues={store?.issues ?? null} apply={apply} openBoard={openBoard} openIssue={openIssue} />;
}

showSidebar(<Sidebar />);
