// The project's open pull requests: in the block of a Project page, in a tab opened with no pull request, and each way the list can fail.
import { Page } from "wings-plugin-ui";
import type { PullsAnswer } from "../src/lib/messages";
import { page as prTab } from "../src/pages/pr";
import { ProjectPulls } from "../src/ui/pulls-list";
import "../src/pages/pr.css";
import { rows } from "./_data";

export { default } from "../src/pages/pulls";
export const page = { place: "view" };
export const queries = { pulls: (): PullsAnswer => ({ rows }) };

const noop = () => {};
const list = (store: "tab" | "block") => <ProjectPulls path="/work/acme/web" name="web" store={store} onOpen={noop} />;
/** What the PR tab shows for a project with no pull request picked: its frame, and the list in a Page. */
const tab = { page: prTab, element: <Page className="pr-page">{list("tab")}</Page> };

export const states = {
  // The block of a Project page, with the list main answers.
  block: { element: list("block") },
  // The block as the page itself mounts it, waiting for the project and then asking main.
  page: {},
  // The list a tab shows when it is opened with no pull request.
  tab,
  // Nothing is open.
  empty: { ...tab, queries: { pulls: (): PullsAnswer => ({ rows: [] }) } },
  // The provider call failed.
  error: {
    ...tab,
    queries: {
      pulls: (): never => {
        throw new Error("gh pr list failed: HTTP 502 from api.github.com. Try again in a minute.");
      },
    },
  },
  // The provider's CLI is not signed in.
  signin: { ...tab, queries: { pulls: (): PullsAnswer => ({ problem: { message: "GitHub CLI isn't signed in, so Wings can't list the pull requests for acme/web.", action: "Sign in" } }) } },
  // Before the main script has answered.
  loading: { ...tab, queries: {} },
};
