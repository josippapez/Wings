// Fixture main for the real native bundles: it answers the queries the PR tab and the project's list ask, with made-up
// data only. Run as a plugin's main, in place of dist/native/main.js, by the native app's tests and screenshots.
const ago = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString();
const url = "https://github.com/acme/web/pull/482";
const args = { provider: "github", url, number: 482, repo: "acme/web", title: "Add retry with backoff to the sync client", cwd: "__ROOT__" };
const status = {
  key: "abc1234", title: args.title, label: "Open", state: "open", tone: "success", icon: "pr-open", changes: "+212 -48 in 9 files",
  checks: { passed: 1, failed: 1, running: 1 }, review: "1 approved, changes requested", failing: [],
  branch: "ada/sync-retry → main", updated: ago(35), merge: "Squash",
  items: [
    { id: "unit", name: "Unit tests", state: "failed", required: true, kind: "Workflow", at: ago(38), seconds: 214, url: "https://github.com/acme/web/actions/runs/1", note: "3 tests failed in src/sync/client.test.ts", run: { job: "1" } },
    { id: "e2e", name: "End-to-end", state: "running", required: true, kind: "Workflow", at: ago(6), seconds: null, url: null, note: "Started 6 min ago" },
    { id: "lint", name: "Lint", state: "passed", required: false, kind: "Workflow", at: ago(40), seconds: 52, url: null, note: null },
  ],
};
const overview = {
  author: "Ada Lovelace", createdAt: ago(60 * 26), status,
  reviewers: [{ name: "Grace Hopper", vote: "approved" }, { name: "Alan Turing", vote: "changes" }],
  body: "## What\n\nThe sync client gave up on the first failed request. It now retries with **exponential backoff**, see [the guide](https://example.com/retries).\n\n- `SyncClient.send` retries up to 5 times\n- Delay doubles from 200 ms\n\n```ts\nconst delay = Math.min(base * 2 ** attempt, 30_000);\n```\n\n| Setting | Default |\n| --- | --- |\n| base | 200 ms |\n",
};
const comments = [
  { id: 1, replyTo: null, path: "src/sync/client.ts", line: 88, side: "additions", author: "Alan Turing", body: "Should the jitter be full or equal?", createdAt: ago(60 * 20), url: url + "#c1", canReply: true, thread: { state: "active", label: "Active" },
    snippet: [{ n: 86, text: "  const wait = Math.min(base * 2 ** attempt, 30_000);" }, { n: 87, text: "  const jitter = wait * (0.5 + Math.random() / 2);", added: true, hit: true }] },
  { id: 2, replyTo: 1, path: "src/sync/client.ts", line: 88, side: "additions", author: "Ada Lovelace", body: "Equal jitter keeps a floor on the delay.", createdAt: ago(60 * 19), url: url + "#c2" },
  { id: 3, replyTo: null, path: null, line: null, side: "additions", author: "ci-bot[bot]", body: "Coverage is 91.2%, down 0.1% from main.", createdAt: ago(60 * 5), url: url + "#c3" },
];
const rows = [
  { id: "github:482", title: args.title, number: "#482", author: "Ada Lovelace", branch: "ada/sync-retry → main", scope: "mine", draft: false, badge: { text: "Open", tone: "success" }, review: "1 approved", checks: { passed: 1, failed: 1, running: 1 }, updated: ago(35), unread: 2, agent: null, args },
  { id: "github:475", title: "web: Migrate the settings page", number: "#475", author: "Grace Hopper", branch: "grace/settings-forms → main", scope: "review", draft: false, badge: { text: "Open", tone: "success" }, review: "Waiting for you", checks: { passed: 5, failed: 0, running: 0 }, updated: ago(480), unread: 0, agent: "working", args: { ...args, number: 475, url: "https://github.com/acme/web/pull/475" } },
];
const data = { pr: { can: { approve: true, vote: false, snippet: false }, overview }, "pr-comments": comments, "pr-pushes": [], pulls: { rows } };
// The same pull request on Azure DevOps: policies as checks (builds that open their run, reviewer and work item policies
// still going, one that didn't apply), votes, the merge strategy, pushes and a resolved thread.
const azureUrl = "https://dev.azure.com/acme/Web/_git/web/pullrequest/482";
const build = (id) => `https://dev.azure.com/acme/Web/_build/results?buildId=${id}`;
const azureItems = [
  { id: "build:9001", name: "web-ci", state: "failed", required: true, kind: "Build", at: ago(31), seconds: 402, url: build(9001), note: "The build failed", failing: { id: "9001", name: "web-ci", build: "9001" }, run: { build: "9001" } },
  { id: "reviewers", name: "Required reviewers", state: "running", required: true, kind: "Required reviewers", at: null, seconds: null, url: null, note: "1 of 2 required reviewers approved. Waiting for Grace Hopper" },
  { id: "build:9002", name: "web-e2e", state: "running", required: true, kind: "Build", at: ago(4), seconds: null, url: build(9002), note: null, run: { build: "9002" } },
  { id: "minimum", name: "Minimum number of reviewers", state: "queued", required: true, kind: "Minimum reviewers", at: null, seconds: null, url: null, note: "1 of 2 approvals" },
  { id: "work-items", name: "Work item linking", state: "queued", required: true, kind: "Work item linking", at: null, seconds: null, url: null, note: "Link a work item to this pull request" },
  { id: "build:9003", name: "web-storybook", state: "queued", required: false, kind: "Build", at: null, seconds: null, url: build(9003), note: "Waiting for the build to start", run: { build: "9003" } },
  { id: "comments", name: "Comment requirements", state: "passed", required: true, kind: "Comment requirements", at: ago(50), seconds: null, url: null, note: null, needs: "unresolved" },
  { id: "status:docs", name: "docs/preview", state: "skipped", required: false, kind: "Status check", at: null, seconds: null, url: null, note: "Doesn't apply to this pull request" },
];
const azureOverview = {
  ...overview,
  reviewers: [{ name: "Grace Hopper", vote: "none" }, { name: "Alan Turing", vote: "approved" }],
  status: { ...status, checks: { passed: 1, failed: 1, running: 5 }, review: "1 approved", merge: "Squash only, pick it when completing", items: azureItems },
};
const commit = (sha, message, minutes) => ({ sha: sha.padEnd(40, "0"), message, author: "Ada Lovelace", at: ago(minutes) });
const azurePushes = [
  { id: 2, author: "Ada Lovelace", at: ago(60 * 3), more: false, created: false, head: "a".repeat(40), base: "b".repeat(40), commits: [commit("929d227", "fix(sync): cap the delay at 30 s", 60 * 3), commit("0c11d9e", "test(sync): cover the Retry-After header", 60 * 3 + 9)] },
  { id: 1, author: "Ada Lovelace", at: ago(60 * 26), more: false, created: true, head: "c".repeat(40), base: "b".repeat(40), commits: [commit("2e70136", "feat(sync): retry with exponential backoff", 60 * 26)] },
];
const azureComments = [
  ...comments.slice(0, 2),
  { id: 4, replyTo: null, path: "src/sync/client.test.ts", line: 14, side: "additions", author: "Grace Hopper", body: "Nit: name this retries on 503.", createdAt: ago(60 * 18), url: azureUrl, canReply: true, thread: { state: "resolved", label: "Resolved" } },
];
const azure = { pr: { can: { approve: false, vote: true, snippet: false }, overview: azureOverview }, "pr-comments": azureComments, "pr-pushes": azurePushes };
const run = {
  url: "https://github.com/acme/web/actions/runs/1",
  jobs: [{ id: "1", name: "Tests", stage: null, state: "failed", seconds: 214, steps: [
    { id: "1", name: "Install dependencies", state: "passed", seconds: 41, log: "1" },
    { id: "2", name: "Unit tests", state: "failed", seconds: 182, log: "2" },
    { id: "3", name: "Publish coverage", state: "skipped", seconds: null, log: null },
  ] }],
};
// The failed step's log: the task's banner between separator lines wider than the log, a folded install whose lines
// put the numbers after it in the hundreds, a stack trace line that wraps, and a link with no space to break at.
const stamp = (s) => `2026-11-06T09:0${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}.000Z `;
const rule = "=".repeat(78);
const log = [
  `${stamp(1)}##[group]Run pnpm test`,
  `${stamp(1)}pnpm test sync`,
  `${stamp(1)}##[endgroup]`,
  `${stamp(1)}${rule}`,
  `${stamp(1)}Task         : Command line`,
  `${stamp(1)}Description  : Run a command line script using Bash on Linux and macOS and cmd.exe on Windows`,
  `${stamp(1)}Version      : 2.250.1`,
  `${stamp(1)}${rule}`,
  `${stamp(1)}Generating script.`,
  `${stamp(2)}##[group]Install dependencies`,
  ...Array.from({ length: 94 }, (_, i) => `${stamp(2 + Math.floor(i / 8))}Progress: resolved ${(i + 1) * 9}, reused ${(i + 1) * 9}, downloaded 0, added ${i * 9}`),
  `${stamp(14)}##[endgroup]`,
  `${stamp(14)}> web@0.0.0 test /home/runner/work/web/web`,
  `${stamp(14)}> vitest run --reporter=verbose --coverage.enabled=true src/sync`,
  ...Array.from({ length: 12 }, (_, i) => `${stamp(15 + i)} \u001b[32m✓\u001b[0m src/sync/module-${i + 1}.test.ts (${8 + i} tests) \u001b[33m${120 + i * 7}ms\u001b[0m`),
  `${stamp(28)} \u001b[31m✗\u001b[0m src/sync/client.test.ts (14 tests | 3 failed)`,
  `${stamp(28)}   \u001b[31m✗\u001b[0m retries with backoff > waits for Retry-After`,
  `${stamp(28)}     \u001b[31mAssertionError: expected 1200 to be 30000 // Object.is equality\u001b[0m`,
  `${stamp(28)}       at processTicksAndRejections (node:internal/process/task_queues:95:5) at runTest (file:///home/runner/work/web/web/node_modules/@vitest/runner/dist/index.js:781:11)`,
  `${stamp(29)}${"=".repeat(150)}`,
  `${stamp(29)}Coverage: https://github.com/acme/web/actions/runs/16893571234/artifacts/3720129876?check_suite_focus=true&pr=482&lines=covered,uncovered,partial`,
  `${stamp(29)}##[error]Process completed with exit code 1.`,
];
const installLog = [
  `${stamp(0)}##[group]Run pnpm install --frozen-lockfile`,
  `${stamp(0)}pnpm install --frozen-lockfile`,
  `${stamp(0)}##[endgroup]`,
  `${stamp(0)}Lockfile is up to date, resolution step is skipped`,
  `${stamp(30)}Packages: +812`,
  `${stamp(40)}Done in 41.2s`,
];
const asked = { "pr-run": () => run, "pr-run-log": (request) => (request?.step?.id === "1" ? installLog : log) };
const respond = (message) => wings.broadcast({ "wings:sdk": 1, ...message });
wings.onBroadcast((m) => {
  if (m?.["wings:sdk"] !== 1) return;
  if (m.kind === "ask") void respond({ kind: "answer", id: m.id, response: asked[m.name]?.(m.request) ?? null });
  if (m.kind === "sub") for (const [name, key] of m.subs) void respond({ kind: "state", name, key, data: (key?.provider === "azure" ? azure : data)[name], at: Date.now() });
});
// A page that subscribed before this listener existed lost its sub. "up" asks every page to subscribe again, as the
// SDK's main does once it serves.
void respond({ kind: "up", boot: "fixture" });
