// The Pull requests block of a Project page: the project's open pull requests, yours first and then the ones
// waiting for your review. The main script owns the lists and their cache; this page asks it, only while Wings is
// in front and the block is on screen, and a click opens the PR in a tab.

import { ProjectGate } from "wings-plugin-ui";
import { ProjectPulls } from "../ui/pulls-list";

export const page = { place: "block" };

export default function Pulls() {
  return (
    <ProjectGate loading="Loading pull requests">
      {(project) => <ProjectPulls path={project.path} name={project.name} store="block" onOpen={(args) => void wings.openView("pr", { as: "tab", args })} />}
    </ProjectGate>
  );
}
