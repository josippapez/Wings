// The project's pull request list when the provider's CLI is not signed in.
import type { PullsAnswer } from "../src/lib/messages";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pages/pr.css";
import { Page } from "wings-plugin-ui";

export const fill = true;
export const queries = { pulls: (): PullsAnswer => ({ problem: { message: "GitHub CLI isn't signed in, so Wings can't list the pull requests for acme/web.", action: "Sign in" } }) };
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
