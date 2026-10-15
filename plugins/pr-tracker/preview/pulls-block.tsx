// The Pull requests block of a Project page.
import type { PullsAnswer } from "../src/lib/messages";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pages/pr.css";
import { rows } from "./_data";

export const place = "view";
export const answers = { pulls: (): PullsAnswer => ({ rows }) };
export default <ProjectPulls path="/work/acme/web" name="web" store="block" onOpen={() => {}} />;
