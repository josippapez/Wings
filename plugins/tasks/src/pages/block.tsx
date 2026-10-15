// The Issues block of a Project page: counts and the project's open issues, compact. Add an issue on the line above,
// open one to read it, or open the board tab for the rest.

import { ProjectGate } from "wings-plugin-ui";
import { BlockView } from "../ui/block-view";
import type { Issue } from "../lib/domain";
import { useIssues } from "../ui/useIssues";
import "./tasks.css";

export const page = { place: "block" };

export default function BlockPage() {
  return (
    <ProjectGate loading="Loading issues" empty={{ description: "Open a project to see its issues." }}>
      {(project) => <ProjectBlock project={project} />}
    </ProjectGate>
  );
}

function ProjectBlock({ project }: { project: WingsProject }) {
  const { store, apply } = useIssues(project.id, project.name);
  return (
    <BlockView
      project={project}
      issues={store?.issues ?? null}
      apply={apply}
      openBoard={() => void wings.openView("board", { as: "tab" })}
      openIssue={(issue: Issue) => void wings.openView("issue", { as: "tab", args: { key: issue.key, projectId: project.id } })}
    />
  );
}
