// The Sprint block when nothing is assigned to you in the running sprint.
import Sprint from "../src/pages/sprint";
import { sprint } from "./_data";

export const place = "view";
export const answers = { sprint: () => ({ ...sprint, items: [], total: 0 }) };
export default <Sprint />;
