// The Sprint block of a Project page with a sprint running, Mine first.
import Sprint from "../src/pages/sprint";
import { sprint } from "./_data";

export const place = "view";
export const answers = { sprint: () => sprint };
export default <Sprint />;
