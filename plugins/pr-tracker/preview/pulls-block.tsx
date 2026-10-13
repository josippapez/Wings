// The Pull requests block of a Project page.
import type { PullsRequest, PullsResponse } from "../src/lib/pulls";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pr.css";
import { rows } from "./_data";

export const place = "view";
export const respond = (m: unknown): PullsResponse | undefined => {
  const { type, rid } = m as PullsRequest;
  return type === "pulls-request" ? { type: "pulls-response", rid, rows } : undefined;
};
export default <ProjectPulls path="/work/acme/web" name="web" store="block" onOpen={() => {}} />;
