// Made-up pull request data for the fixtures in this folder, shaped like what pr.tsx loads.
import type { Comment, Overview, PrArgs } from "../src/lib/types";
import type { PrViewProps } from "../src/ui/pr-view";
import type { PullRow } from "../src/lib/pulls";
import type { Push } from "../src/lib/pushes";
import { diffWindow, snippetOf } from "../src/lib/snippet";

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
  { id: 1, replyTo: null, path: "src/sync/client.ts", line: 88, side: "additions", range: { start: 88, end: 88, iteration: 2 }, author: "Alan Turing", body: "Should the jitter be full or equal? Full jitter spreads the load better.\n<details>\n<summary>Context & sources</summary>\n\n**Why:** a fixed floor lets every client retry in step.\n\n- docs/standards/retries.md\n</details>\n\n[ISSUE_KEY]: AbC-123\n[PROJECT_KEY]: acme_web", createdAt: ago(60 * 20), url: "https://github.com/acme/web/pull/482#c1", canReply: true, thread: { state: "active", label: "Active" } },
  { id: 2, replyTo: 1, path: "src/sync/client.ts", line: 88, side: "additions", author: "Ada Lovelace", body: "Equal jitter keeps a floor on the delay, which I wanted. Happy to switch if you disagree.", createdAt: ago(60 * 19), url: "https://github.com/acme/web/pull/482#c2" },
  { id: 3, replyTo: null, path: "src/sync/client.test.ts", line: 14, range: { start: 14, end: 14, iteration: 1 }, side: "additions", author: "Grace Hopper", body: "Nit: name this `retries on 503`.", createdAt: ago(60 * 18), url: "https://github.com/acme/web/pull/482#c3", canReply: true, thread: { state: "resolved", label: "Resolved" } },
  { id: 5, replyTo: null, path: "src/sync/client.ts", line: 41, range: { start: 40, end: 41, iteration: 2 }, side: "additions", author: "Project Build Service (Acme)", body: "**Retry guardrail**\n\n**Major: Violation:** docs/standards/retries.md\n\nThe delay is computed inline instead of through the shared backoff helper, so the jitter standard cannot be changed in one place.\n\n- Use `backoff()` from `src/sync/backoff.ts`\n- Keep the cap at 30 s\n\n<details>\n<summary>Context & sources</summary>\n\n**Standard:** every retry loop goes through the shared helper.\n\n- docs/standards/retries.md\n- docs/standards/jitter.md\n</details>\n\n[ISSUE_KEY]: AbC-123", createdAt: ago(60 * 21), url: "https://github.com/acme/web/pull/482#c5", canReply: true, thread: { state: "active", label: "Active" } },
  { id: 4, replyTo: null, path: null, line: null, side: "additions", author: "ci-bot[bot]", body: "Coverage is 91.2%, down 0.1% from main.", createdAt: ago(60 * 5), url: "https://github.com/acme/web/pull/482#c4" },
];

/** A hunk as GitHub gives it in a review comment: from its start down to the commented line. */
const HUNK = [
  "@@ -80,7 +80,10 @@ export class SyncClient {",
  "   async send(request: Request, attempt = 0): Promise<Response> {",
  "     const res = await fetch(request);",
  "     if (res.status < 500 || attempt >= 5) return res;",
  "-    const wait = this.base * 2 ** attempt;",
  "-    await delay(wait);",
  "+    const wait = Math.min(this.base * 2 ** attempt, 30_000);",
  "+    const jitter = wait * (0.5 + Math.random() / 2);",
  "+    await delay(res.headers.get(\"Retry-After\") ? retryAfter(res) : jitter);",
  "     return this.send(request, attempt + 1);",
].join("\n");

/** The same conversation on GitHub, whose comments carry their code: a thread on added lines, one on a removed line, and a reply with a fenced block. */
export const githubComments: Comment[] = comments.map((c) => {
  if (c.id === 1) return { ...c, range: undefined, line: 85, snippet: diffWindow(HUNK, "additions", 84, 85)! };
  if (c.id === 3) return { ...c, range: undefined, line: 83, side: "deletions", snippet: diffWindow(HUNK, "deletions", 83)! };
  if (c.id === 5) return { ...c, range: undefined, line: 85, snippet: diffWindow(HUNK, "additions", 85)! };
  if (c.id === 2) return { ...c, body: "Equal jitter keeps a floor on the delay, which I wanted:\n\n```ts\nconst jitter = wait * (0.5 + Math.random() / 2);\n```\n\nHappy to switch if you disagree." };
  return c;
});

const commit = (sha: string, message: string, author: string, minutes: number) => ({ sha: sha.padEnd(40, "0"), message, author, at: ago(minutes) });

/** Two pushes, the newer with more commits than it first lists. */
export const pushes: Push[] = [
  { id: 2, author: "Ada Lovelace", at: ago(60 * 22), more: false, created: false, head:"a".repeat(40), base: "b".repeat(40), commits: [
    commit("929d227", "feat(sync): start chunks as the per-minute budget allows", "Ada Lovelace", 60 * 22),
    commit("eaa4b73", "refactor(sync): remove the old migration tooling", "Ada Lovelace", 60 * 22 + 11),
    commit("87855cd", "refactor(sync): share duplicated helpers and drop dead code", "Ada Lovelace", 60 * 22 + 15),
    commit("4e57f05", "refactor(sync): drop the unused coverage totals, no-op build gate and package metadata", "Ada Lovelace", 60 * 22 + 30),
    commit("5a16c28", "refactor(sync): always chunk both reviews and make unset settings constants", "Ada Lovelace", 60 * 22 + 40),
    commit("0c11d9e", "test(sync): cover the Retry-After header", "Ada Lovelace", 60 * 22 + 55),
  ] },
  { id: 1, author: "Ada Lovelace", at: ago(60 * 26), more: false, created: true, head:"c".repeat(40), base: "b".repeat(40), commits: [
    commit("2e70136", "feat(sync): retry with exponential backoff", "Ada Lovelace", 60 * 26),
    commit("f563a17", "docs(sync): describe the retry settings", "Ada Lovelace", 60 * 26 + 12),
  ] },
];

const FILE = `import { delay } from "./time";

export class SyncClient {
  constructor(private base = 200) {}

  async send(request: Request, attempt = 0): Promise<Response> {
    const res = await fetch(request);
    if (res.status < 500 || attempt >= 5) return res;
    const wait = Math.min(this.base * 2 ** attempt, 30_000);
    const jitter = wait * (0.5 + Math.random() / 2);
    await delay(res.headers.get("Retry-After") ? retryAfter(res) : jitter);
    return this.send(request, attempt + 1);
  }
}
`.repeat(8);

export const onSnippet: PrViewProps["onSnippet"] = async (root) => snippetOf(FILE, root.range?.start ?? 1, root.range?.end, new Set([85, 86, 87, 88, 89]));

/** What PrView is given once everything has loaded. */
export const props: PrViewProps = {
  args,
  overview,
  comments,
  pushes,
  onSnippet,
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
  { id: "unit", name: "Unit tests", state: "failed", required: true, kind: "Workflow", at: ago(38), seconds: 214, url: "https://github.com/acme/web/actions/runs/2", note: "3 tests failed in src/sync/client.test.ts", failing: { id: "2", name: "Unit tests", summary: "3 tests failed" }, run: { job: 2 } },
  { id: "e2e", name: "End-to-end (chromium)", state: "running", required: true, kind: "Workflow", at: ago(6), seconds: null, url: null, note: "Started 6 min ago" },
  { id: "e2e-ff", name: "End-to-end (firefox)", state: "queued", required: false, kind: "Workflow", at: null, seconds: null, url: null, note: "Waiting for a runner" },
  { id: "types", name: "Type check", state: "passed", required: true, kind: "Workflow", at: ago(41), seconds: 95, url: "https://github.com/acme/web/actions/runs/3", note: null },
  { id: "build", name: "Build", state: "passed", required: true, kind: "Workflow", at: ago(39), seconds: 132, url: "https://github.com/acme/web/actions/runs/4", note: null, run: { job: 4 } },
  { id: "cov", name: "Coverage", state: "passed", required: false, kind: "Status check", at: ago(37), seconds: null, url: null, note: "91.2% of lines" },
  { id: "reviews", name: "Required reviewers", state: "running", required: true, kind: "Policy", at: null, seconds: null, url: null, note: "1 of 2 required reviewers approved" },
  { id: "threads", name: "Comment requirements", state: "failed", required: true, kind: "Policy", at: null, seconds: null, url: null, note: null, needs: "unresolved" },
  { id: "docs", name: "Docs preview", state: "skipped", required: false, kind: "Status check", at: null, seconds: null, url: null, note: "No docs changed" },
];

export const withChecks: PrViewProps = { ...props, overview: { ...overview, status: { ...overview.status, items } }, onLoadFailure: async () => [], onLoadRun: async () => ({ url: null, jobs: [] }), onLoadRunLog: async () => [] };
