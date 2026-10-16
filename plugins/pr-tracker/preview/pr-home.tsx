// The "pr" tab as the page itself opens it with no pull request: the project's list, asked of main.
import type { PullsAnswer } from "../src/lib/messages";
import PrPage from "../src/pages/pr";
import { rows } from "./_data";

export const fill = true;
export const queries = { pulls: (): PullsAnswer => ({ rows }) };
export default PrPage;
