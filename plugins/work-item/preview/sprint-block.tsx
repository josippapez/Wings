// The Sprint block of a Project page: the whole sprint, then it falls back to the empty cases through `respond`.
import type { SprintRequest, SprintResponse } from "../src/lib/sections";
import { Sprint } from "../src/ui/sprint-view";
import { sprint } from "./_data";

export const place = "view";
export const respond = (m: unknown): SprintResponse | undefined => {
  const { type, rid } = m as SprintRequest;
  return type === "sprint-request" ? { ...sprint, rid } : undefined;
};
export default <Sprint />;
