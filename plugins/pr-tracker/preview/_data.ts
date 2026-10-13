// Made-up pull request data for the fixtures in this folder, shaped like what pr.tsx loads.
import type { Overview } from "../src/lib/refs";
import type { Comment, PrArgs } from "../src/lib/types";
import type { PrViewProps } from "../src/ui/pr-view";
import type { PullRow } from "../src/lib/pulls";

const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

export const args: PrArgs = {
  provider: "github",
  url: "https://github.com/acme/web/pull/482",
  number: 482,
  repo: "acme/web",
  title: "Add retry with backoff to the sync client",
  cwd: "/work/acme/web",
};

export const overview: Overview = {
  author: "Ada Lovelace",
  createdAt: ago(60 * 26),
  reviewers: [
    { name: "Grace Hopper", vote: "approved" },
    { name: "Alan Turing", vote: "changes" },
    { name: "Edsger Dijkstra", vote: "none" },
  ],
  body: `## What

The sync client gave up on the first failed request. It now retries with exponential backoff.

## Changes

- \`SyncClient.send\` retries up to 5 times
- Delay doubles from 200 ms, with jitter
- A \`Retry-After\` header wins over the computed delay

\`\`\`ts
const delay = Math.min(base * 2 ** attempt, 30_000) * (0.5 + Math.random() / 2);
\`\`\`

## Testing

1. Run \`pnpm test sync\`
2. Kill the server mid-request and watch the client recover
`,
  status: {
    key: "abc1234",
    title: args.title,
    label: "Open",
    tone: "success",
    icon: "pr-open",
    checks: { passed: 7, failed: 1, running: 2 },
    review: "1 approved, changes requested",
    failing: [],
    changes: "+212 -48 in 9 files",
    branch: "ada/sync-retry → main",
    updated: ago(35),
    merge: "Squash",
  },
};

export const comments: Comment[] = [
  { id: 1, replyTo: null, path: "src/sync/client.ts", line: 88, side: "additions", author: "Alan Turing", body: "Should the jitter be full or equal? Full jitter spreads the load better.", createdAt: ago(60 * 20), url: "https://github.com/acme/web/pull/482#c1", canReply: true, thread: { state: "active", label: "Active" } },
  { id: 2, replyTo: 1, path: "src/sync/client.ts", line: 88, side: "additions", author: "Ada Lovelace", body: "Equal jitter keeps a floor on the delay, which I wanted. Happy to switch if you disagree.", createdAt: ago(60 * 19), url: "https://github.com/acme/web/pull/482#c2" },
  { id: 3, replyTo: null, path: "src/sync/client.test.ts", line: 14, side: "additions", author: "Grace Hopper", body: "Nit: name this `retries on 503`.", createdAt: ago(60 * 18), url: "https://github.com/acme/web/pull/482#c3", canReply: true, thread: { state: "resolved", label: "Resolved" } },
  { id: 4, replyTo: null, path: null, line: null, side: "additions", author: "ci-bot[bot]", body: "Coverage is 91.2%, down 0.1% from main.", createdAt: ago(60 * 5), url: "https://github.com/acme/web/pull/482#c4" },
];

/** What PrView is given once everything has loaded. */
export const props: PrViewProps = {
  args,
  overview,
  comments,
  error: null,
  notice: null,
  busy: null,
  canReply: true,
  onRefresh: () => {},
  onRun: () => {},
  onReply: async () => {},
  onApprove: async () => {},
};

const row = (n: number, title: string, branch: string, author: string, scope: PullRow["scope"], extra: Partial<PullRow> = {}): PullRow => ({
  id: `acme/web#${n}`,
  title,
  number: `#${n}`,
  author,
  branch,
  scope,
  draft: false,
  badge: { text: "Open", tone: "success" },
  review: "",
  checks: { passed: 5, failed: 0, running: 0 },
  updated: ago(n),
  unread: 0,
  agent: null,
  args: { ...args, number: n, title, url: `https://github.com/acme/web/pull/${n}` },
  ...extra,
});

export const rows: PullRow[] = [
  row(482, "Add retry with backoff to the sync client", "ada/sync-retry → main", "Ada Lovelace", "mine", { review: "1 approved", unread: 2, checks: { passed: 7, failed: 1, running: 2 } }),
  row(479, "[web] Fix the date picker on small screens", "ada/date-picker → main", "Ada Lovelace", "mine", { draft: true, badge: { text: "Draft", tone: "neutral" }, agent: "working" }),
  row(475, "Migrate the settings page to the new form components", "grace/settings-forms → main", "Grace Hopper", "review", { review: "Waiting for you" }),
  row(470, "Bump the build toolchain", "alan/toolchain → main", "Alan Turing", "other", { checks: { passed: 4, failed: 0, running: 1 } }),
];

/** Every check of the pull request, so the Checks tab has something to group. */
export const items: NonNullable<Overview["status"]["items"]> = [
  { id: "lint", name: "Lint", state: "passed", required: true, kind: "Workflow", at: ago(40), seconds: 48, url: "https://github.com/acme/web/actions/runs/1", note: null },
  { id: "unit", name: "Unit tests", state: "failed", required: true, kind: "Workflow", at: ago(38), seconds: 214, url: "https://github.com/acme/web/actions/runs/2", note: "3 tests failed in src/sync/client.test.ts", failing: { id: "2", name: "Unit tests", summary: "3 tests failed" } },
  { id: "e2e", name: "End-to-end (chromium)", state: "running", required: true, kind: "Workflow", at: ago(6), seconds: null, url: null, note: "Started 6 min ago" },
  { id: "e2e-ff", name: "End-to-end (firefox)", state: "queued", required: false, kind: "Workflow", at: null, seconds: null, url: null, note: "Waiting for a runner" },
  { id: "types", name: "Type check", state: "passed", required: true, kind: "Workflow", at: ago(41), seconds: 95, url: "https://github.com/acme/web/actions/runs/3", note: null },
  { id: "build", name: "Build", state: "passed", required: true, kind: "Workflow", at: ago(39), seconds: 132, url: "https://github.com/acme/web/actions/runs/4", note: null },
  { id: "cov", name: "Coverage", state: "passed", required: false, kind: "Status check", at: ago(37), seconds: null, url: null, note: "91.2% of lines" },
  { id: "reviews", name: "Required reviewers", state: "running", required: true, kind: "Policy", at: null, seconds: null, url: null, note: "1 of 2 required reviewers approved" },
  { id: "threads", name: "Comment requirements", state: "failed", required: true, kind: "Policy", at: null, seconds: null, url: null, note: null, needs: "unresolved" },
  { id: "docs", name: "Docs preview", state: "skipped", required: false, kind: "Status check", at: null, seconds: null, url: null, note: "No docs changed" },
];

export const withChecks: PrViewProps = { ...props, overview: { ...overview, status: { ...overview.status, items } }, onLoadFailure: async () => [] };
