// The project's pull request list when nothing is open.
import type { PullsRequest, PullsResponse } from "../src/lib/pulls";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pages/pr.css";
import { Page } from "wings-plugin-ui";

export const fill = true;
export const respond = (m: unknown): PullsResponse | undefined => {
  const { type, rid } = m as PullsRequest;
  return type === "pulls-request" ? { type: "pulls-response", rid, rows: [] } : undefined;
};
export default (
  <Page className="pr-page">
    <ProjectPulls path="/work/acme/web" name="web" store="tab" onOpen={() => {}} />
  </Page>
);
