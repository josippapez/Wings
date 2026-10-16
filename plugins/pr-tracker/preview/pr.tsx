// The real PR page, fed by main's queries: a pull request, a long conversation, an Azure DevOps one with its pushes, and a
// host that needs signing in.
import type { PrValue } from "../src/lib/messages";
import type { Comment, PrArgs } from "../src/lib/types";
import { PrView } from "../src/ui/pr-view";
import "../src/pages/pr.css";
import { args, comments, overview, props, pushes } from "./_data";

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

export const states = {
  // Opened straight on a pull request.
  page: { args, queries: { pr: value(), "pr-comments": comments, "pr-pushes": [] } },
  // A conversation over 64 KB, which main sends in parts.
  "page-big": { args, queries: { pr: value(), "pr-comments": many, "pr-pushes": [] } },
  // Azure DevOps: votes instead of approving, and pushes between the threads.
  "page-azure": { args: azure, queries: { pr: value({ approve: false, vote: true, snippet: true }), "pr-comments": comments, "pr-pushes": pushes } },
  // gh isn't signed in: the tab offers to sign in, as the pane's badge does.
  signin: {
    element: <PrView {...props} overview={null} comments={null} problem={{ message: "Sign in to GitHub to see this branch's pull request.", action: "Sign in" }} onProblem={() => {}} />,
  },
};
