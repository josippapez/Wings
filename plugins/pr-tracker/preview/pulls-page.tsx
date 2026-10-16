// The Pull requests block as the page itself mounts it, waiting for the project and then asking main.
import type { PullsAnswer } from "../src/lib/messages";
import Pulls from "../src/pages/pulls";
import { rows } from "./_data";

export const place = "view";
export const queries = { pulls: (): PullsAnswer => ({ rows }) };
export default Pulls;
