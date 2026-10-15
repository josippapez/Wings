// The Issue tab: one issue of a project, opened with args { key, projectId, trail }; trail is the issues opened before it
// in this tab, for the breadcrumb back. Edit its title, status, description and
// links, add a note, resume the last Claude session that worked on it, or start a new agent on it.

import { useEffect, useState } from "react";
import { Empty, Loading } from "wings-plugin-ui";
import { IssueView } from "../ui/issue-view";
import { agentCommand, findIssue, lastSession, resumeCommand, trailTo, TRAIL_LENGTH, type Op } from "../lib/issues";
import { useIssues } from "../lib/useIssues";
import "./tasks.css";

export const page = { place: "tab", fill: true };

export default function IssuePage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => void wings.project().then(setProject), []);
  if (project === undefined) return <Loading>Loading issue</Loading>;
  if (!project) return <Empty title="No project" description="Open a project to see its issues." />;
  return <ProjectIssue key={project.id} project={project} />;
}

function ProjectIssue({ project }: { project: WingsProject }) {
  const args = wings.viewArgs<{ key?: string; projectId?: string; trail?: unknown }>();
  const issueKey = args?.key ?? "";
  const trail = Array.isArray(args?.trail) ? args.trail.filter((k): k is string => typeof k === "string").slice(-TRAIL_LENGTH) : [];
  const projectId = args?.projectId ?? project.id;
  const openIssue = (key: string, newTrail: string[], newTab = false) => void wings.openView("issue", { as: "tab", args: { key, projectId, trail: newTrail }, newTab }).catch(fail);
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
      trail={trail}
      onBackTo={(i) => openIssue(trail[i], trail.slice(0, i))}
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
      onOpenIssue={(key, newTab) => openIssue(key, trailTo(trail, issueKey, key), newTab)}
    />
  );
}
