// The project's open pull requests as a tab shows them (a "pr" tab opened with no pull request).
import type { PullsAnswer } from "../src/lib/messages";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pages/pr.css";
import { Page } from "wings-plugin-ui";
import { rows } from "./_data";

export const fill = true;
export const queries = { pulls: (): PullsAnswer => ({ rows }) };
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
