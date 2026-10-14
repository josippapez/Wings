// The Issue tab: one issue of a project, opened with args { key, projectId }. Edit its title, status, description and
// links, add a note, resume the last Claude session that worked on it, or start a new agent on it.

import { useEffect, useState } from "react";
import { Empty, Loading, showTab } from "wings-plugin-ui";
import { IssueView } from "./lib/issue-view";
import { agentCommand, findIssue, lastSession, resumeCommand, type Op } from "./lib/issues";
import { useIssues } from "./lib/useIssues";
import "./tasks.css";

function IssuePage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => void wings.project().then(setProject), []);
  if (project === undefined) return <Loading>Loading issue</Loading>;
  if (!project) return <Empty title="No project" description="Open a project to see its issues." />;
  return <ProjectIssue key={project.id} project={project} />;
}

function ProjectIssue({ project }: { project: WingsProject }) {
  const args = wings.viewArgs<{ key?: string; projectId?: string }>();
  const issueKey = args?.key ?? "";
  const { store, apply } = useIssues(args?.projectId ?? project.id, project.name);
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => setError(e instanceof Error ? e.message : String(e));
  const send = (op: Op) => apply(op).then(() => setError(null), fail);
  const issue = store ? findIssue(store, issueKey) : undefined;

  const open = (command: string, args?: string[]) => wings.openPane({ command, args, cwd: project.path }).then(() => setError(null), fail);
  return (
    <IssueView
      project={project}
      issue={issue}
      issues={store?.issues ?? []}
      issueKey={issueKey}
      error={error}
      send={send}
      onBack={() => void wings.openView("board", { as: "tab" })}
      onResume={() => {
        const last = issue && lastSession(issue);
        if (last) void Promise.resolve().then(() => open(resumeCommand(last.id))).catch(fail);
      }}
      onStart={() =>
        void Promise.resolve()
          .then(() => {
            const { command, args } = agentCommand(issueKey);
            return open(command, args);
          })
          .catch(fail)
      }
      onOpenUrl={(url) => void wings.openUrl(url).catch(fail)}
      onOpenIssue={(key, newTab) => void wings.openView("issue", { as: "tab", args: { key, projectId: args?.projectId ?? project.id }, newTab }).catch(fail)}
    />
  );
}

showTab(<IssuePage />, { fill: true });
