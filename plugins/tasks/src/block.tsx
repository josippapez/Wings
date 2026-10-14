// The Issues block of a Project page: counts and the project's open issues, compact. Add an issue on the line above,
// open one to read it, or open the board tab for the rest.

import { useEffect, useRef, useState } from "react";
import { Empty, Loading, showView } from "wings-plugin-ui";
import { BlockView } from "./lib/block-view";
import type { Issue } from "./lib/issues";
import { useIssues } from "./lib/useIssues";
import "./tasks.css";

/** Tells Wings how tall the content is, so the block fits it. */
function useFit(ref: React.RefObject<HTMLElement | null>) {
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = new ResizeObserver(() => void wings.setHeight(Math.ceil(el.getBoundingClientRect().height)));
    fit.observe(el);
    return () => fit.disconnect();
  }, [ref]);
}

function BlockPage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => void wings.project().then(setProject), []);
  useFit(root);
  let body;
  if (project === undefined) body = <Loading>Loading issues</Loading>;
  else if (!project) body = <Empty title="No project" description="Open a project to see its issues." />;
  else body = <ProjectBlock key={project.id} project={project} />;
  return <div ref={root}>{body}</div>;
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

showView(<BlockPage />);
