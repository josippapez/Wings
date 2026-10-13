// The project's pull request list when the provider's CLI is not signed in.
import type { PullsRequest, PullsResponse } from "../src/lib/pulls";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pr.css";
import { Page } from "wings-plugin-ui";

export const fill = true;
export const respond = (m: unknown): PullsResponse | undefined => {
  const { type, rid } = m as PullsRequest;
  return type === "pulls-request" ? { type: "pulls-response", rid, problem: { message: "GitHub CLI isn't signed in, so Wings can't list the pull requests for acme/web.", action: "Sign in" } } : undefined;
};
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
