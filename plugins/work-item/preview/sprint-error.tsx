// The Sprint block when Azure DevOps could not be read.
import type { SprintRequest, SprintResponse } from "../src/lib/sections";
import Sprint from "../src/pages/sprint";

export const place = "view";
export const respond = (m: unknown): SprintResponse | undefined => {
  const { type, rid } = m as SprintRequest;
  return type === "sprint-request" ? { type: "sprint-response", rid, error: "az boards query failed: Please run 'az login' to set up an account." } : undefined;
};
export default <Sprint />;
