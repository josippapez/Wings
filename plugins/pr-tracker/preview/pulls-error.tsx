// The project's pull request list when the provider call failed.
import type { PullsAnswer } from "../src/lib/messages";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pages/pr.css";
import { Page } from "wings-plugin-ui";

export const fill = true;
export const answers = {
  pulls: (): PullsAnswer => {
    throw new Error("gh pr list failed: HTTP 502 from api.github.com. Try again in a minute.");
  },
};
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
