// The project's pull request list before the main script has answered.
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pr.css";
import { Page } from "wings-plugin-ui";

export const fill = true;
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
