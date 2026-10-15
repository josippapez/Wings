// The Issues block of a Project page: counts and the project's open issues, compact. Add an issue on the line above,
// open one to read it, or open the board tab for the rest.

import { useEffect, useState } from "react";
import { Empty, Loading } from "wings-plugin-ui";
import { BlockView } from "../ui/block-view";
import type { Issue } from "../lib/issues";
import { useIssues } from "../lib/useIssues";
import "./tasks.css";

export const page = { place: "block" };


export default function BlockPage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => void wings.project().then(setProject), []);
  let body;
  if (project === undefined) body = <Loading>Loading issues</Loading>;
  else if (!project) body = <Empty title="No project" description="Open a project to see its issues." />;
  else body = <ProjectBlock key={project.id} project={project} />;
  return <div>{body}</div>;
}

function ProjectBlock({ project }: { project: WingsProject }) {
  const { store, apply } = useIssues(project.id, project.name);
  return (
    <BlockView
      project={project}
      issues={store?.issues ?? null}
      error={null}
      apply={apply}
      openBoard={() => void wings.openView("board", { as: "tab" })}
      openIssue={(issue: Issue) => void wings.openView("issue", { as: "tab", args: { key: issue.key, projectId: project.id } })}
    />
  );
}
