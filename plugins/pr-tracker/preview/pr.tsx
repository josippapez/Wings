// The PR tab: a pull request on each of its tabs (through main's queries, or the view alone), a long conversation, an Azure
// DevOps one with its pushes, while it loads, when it can't be read, a host that needs signing in, and the project's list a
// tab with no pull request starts on.
import type { PrValue, PullsAnswer } from "../src/lib/messages";
import type { Comment, PrArgs } from "../src/lib/types";
import { PrView } from "../src/ui/pr-view";
import "../src/pages/pr.css";
import { args, comments, overview, props, pushes, rows, withChecks } from "./_data";

export { default, page } from "../src/pages/pr";

const value = (can: Partial<PrValue["can"]> = {}): PrValue => ({ overview, can: { approve: true, vote: false, snippet: false, ...can } });

/** 400 threads: far more than one broadcast carries. */
const many: Comment[] = Array.from({ length: 400 }, (_, i) => ({
  ...comments[0],
  id: 1000 + i,
  createdAt: new Date(Date.parse(comments[0].createdAt!) + i * 60_000).toISOString(),
  body: `Comment ${i + 1}: ${"Should the retry wait for the server's Retry-After header before it backs off again? ".repeat(2)}`,
  thread: { state: i % 3 ? "resolved" : "active", label: i % 3 ? "Resolved" : "Active" },
}));

const azure: PrArgs = {
  provider: "azure",
  url: "https://dev.azure.com/acme/Web/_git/web/pullrequest/482",
  number: 482,
  repo: "Web/web",
  title: args.title,
  cwd: args.cwd,
  remote: { kind: "azure", org: "https://dev.azure.com/acme", project: "Web", repo: "web" },
};

const back = () => {};

export const states = {
  // Opened straight on a pull request.
  page: { args, queries: { pr: value(), "pr-comments": comments, "pr-pushes": [] } },
  // A conversation over 64 KB, which main sends in parts.
  "page-big": { args, queries: { pr: value(), "pr-comments": many, "pr-pushes": [] } },
  // Azure DevOps: votes instead of approving, and pushes between the threads.
  "page-azure": { args: azure, queries: { pr: value({ approve: false, vote: true, snippet: true }), "pr-comments": comments, "pr-pushes": pushes } },
  // Opened from the project's list: a breadcrumb back in the toolbar.
  picked: { element: <PrView {...props} onBack={back} /> },
  // Opened straight on a pull request: no breadcrumb.
  direct: { element: <PrView {...props} /> },
  // Its Files tab.
  files: { element: <PrView {...props} initialTab="files" onBack={back} /> },
  // Its Checks tab: every check and policy, grouped by how it stands.
  checks: { element: <PrView {...withChecks} initialTab="checks" onBack={back} /> },
  // No description, checks or comments yet.
  empty: { element: <PrView {...props} comments={[]} overview={{ ...props.overview!, body: "", reviewers: [], status: { ...props.overview!.status, review: "No reviews yet", checks: { passed: 0, failed: 0, running: 0 }, changes: null } }} /> },
  // While its status and conversation load.
  loading: { element: <PrView {...props} overview={null} comments={null} onBack={back} /> },
  // When it could not be loaded.
  error: { element: <PrView {...props} overview={null} comments={null} error="gh pr view failed: could not resolve to a pull request with the number 482 in acme/web. Run gh auth status to check you are signed in." onBack={back} /> },
  // As the tab opens with no pull request: the project's list, asked of main.
  home: { queries: { pulls: (): PullsAnswer => ({ rows }) } },
  // gh isn't signed in: the tab offers to sign in, as the pane's badge does.
  signin: {
    element: <PrView {...props} overview={null} comments={null} problem={{ message: "Sign in to GitHub to see this branch's pull request.", action: "Sign in" }} onProblem={() => {}} />,
  },
};
