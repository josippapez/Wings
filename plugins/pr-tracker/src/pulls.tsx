// The Pull requests block of a Project page: the project's open pull requests, yours first and then the ones
// waiting for your review. The main script owns the lists and their cache; this page asks it, only while Wings is
// in front and the block is on screen, and a click opens the PR in a tab.

import { useEffect, useRef, useState } from "react";
import { Loading, showView, Text } from "wings-plugin-ui";
import { ProjectPulls } from "./ui/pulls-list";

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

function Pulls() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    void wings.project().then(setProject);
  }, []);
  useFit(root);
  return <div ref={root}>{project === undefined ? <Loading>Loading pull requests</Loading> : project ? <ProjectPulls path={project.path} name={project.name} store="block" onOpen={(args) => void wings.openView("pr", { as: "tab", args })} /> : <Text tone="muted">No project.</Text>}</div>;
}

showView(<Pulls />);
