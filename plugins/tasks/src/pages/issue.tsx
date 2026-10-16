// The Issue tab: one issue of a project, opened with args { key, projectId, trail }; trail is the issues opened before it
// in this tab, for the breadcrumb back. Edit its title, status, description and
// links, add a note, resume the last Claude session that worked on it, or start a new agent on it.

import { ProjectGate, useAction, useViewArgs } from "wings-plugin-ui";
import { IssueView } from "../ui/issue-view";
import { lastSession } from "../lib/actor";
import { agentCommand, resumeCommand } from "../lib/commands";
import type { Op } from "../lib/domain";
import { findIssue, trailTo, TRAIL_LENGTH } from "../lib/orderings";
import { useIssues } from "../ui/useIssues";
import { useTodos } from "../ui/useTodos";
import "./tasks.css";

export const page = { place: "tab", fill: true };

export default function IssuePage() {
  return (
    <ProjectGate loading="Loading issue" empty={{ description: "Open a project to see its issues." }}>
      {(project) => <ProjectIssue project={project} />}
    </ProjectGate>
  );
}

interface Args {
  key: string;
  projectId?: string;
  trail: string[];
}

/** The args the tab was opened with. */
const argsOf = (raw: unknown): Args | null => {
  if (typeof raw !== "object") return null;
  const { key, projectId, trail } = raw as { key?: unknown; projectId?: unknown; trail?: unknown };
  return {
    key: typeof key === "string" ? key : "",
    projectId: typeof projectId === "string" ? projectId : undefined,
    trail: Array.isArray(trail) ? trail.filter((k): k is string => typeof k === "string").slice(-TRAIL_LENGTH) : [],
  };
};

function ProjectIssue({ project }: { project: WingsProject }) {
  const args = useViewArgs(argsOf);
  const issueKey = args?.key ?? "";
  const trail = args?.trail ?? [];
  const projectId = args?.projectId ?? project.id;
  const { store, apply } = useIssues(projectId, project.name);
  const todos = useTodos(projectId) ?? [];
  const action = useAction();
  const send = (op: Op) => action.run(() => apply(op));
  const issue = store ? findIssue(store, issueKey) : undefined;
  const openIssue = (key: string, newTrail: string[], newTab = false) => void action.run(() => wings.openView("issue", { as: "tab", args: { key, projectId, trail: newTrail }, newTab }));

  return (
    <IssueView
      project={project}
      issue={issue}
      issues={store?.issues ?? []}
      todos={todos}
      issueKey={issueKey}
      error={action.error}
      apply={apply}
      send={send}
      onBack={() => void wings.openView("board", { as: "tab" })}
      trail={trail}
      onBackTo={(i) => openIssue(trail[i], trail.slice(0, i))}
      onResume={() => {
        const last = issue && lastSession(issue);
        if (last) void action.run(() => wings.openPane({ command: resumeCommand(last.id), cwd: project.path }));
      }}
      onStart={() => {
        if (issue) void action.run(() => wings.openPane({ ...agentCommand(issue), cwd: project.path }));
      }}
      onOpenUrl={(url) => void action.run(() => wings.openUrl(url))}
      onOpenIssue={(key, newTab) => openIssue(key, trailTo(trail, issueKey, key), newTab)}
    />
  );
}
