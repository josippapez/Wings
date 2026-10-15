// The project's pull request list when nothing is open.
import type { PullsAnswer } from "../src/lib/messages";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pages/pr.css";
import { Page } from "wings-plugin-ui";

export const fill = true;
export const answers = { pulls: (): PullsAnswer => ({ rows: [] }) };
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
