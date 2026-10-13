// The Project info tab: facts and sections with room to read and edit them.

import { Empty, Loading, Page, showTab, Toolbar } from "wings-plugin-ui";
import { useProject } from "./lib/client";
import { InfoPage } from "./lib/Info";

function Tab() {
  const project = useProject();
  return (
    <Page toolbar={<Toolbar title={project ? `Project info: ${project.name}` : "Project info"} />}>
      {project === undefined ? <Loading>Loading</Loading> : project ? <InfoPage key={project.id} projectId={project.id} name={project.name} /> : <Empty title="No project" description="Open this from a project." />}
    </Page>
  );
}

showTab(<Tab />, { fill: true });
