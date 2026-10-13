// The project's pull request list when the provider call failed.
import type { PullsRequest, PullsResponse } from "../src/lib/pulls";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pr.css";
import { Page } from "wings-plugin-ui";

export const fill = true;
export const respond = (m: unknown): PullsResponse | undefined => {
  const { type, rid } = m as PullsRequest;
  return type === "pulls-request" ? { type: "pulls-response", rid, error: "gh pr list failed: HTTP 502 from api.github.com. Try again in a minute." } : undefined;
};
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
