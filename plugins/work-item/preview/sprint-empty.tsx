// The Sprint block when nothing is assigned to you in the running sprint.
import type { SprintRequest, SprintResponse } from "../src/lib/sections";
import { Sprint } from "../src/ui/sprint-view";
import { sprint } from "./_data";

export const place = "view";
export const respond = (m: unknown): SprintResponse | undefined => {
  const { type, rid } = m as SprintRequest;
  return type === "sprint-request" ? { ...sprint, rid, items: [], total: 0 } : undefined;
};
export default <Sprint />;
