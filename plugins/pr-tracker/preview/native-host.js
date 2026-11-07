// Fixture main for the real native bundles: it answers the queries the PR tab and the project's list ask, with made-up
// data only. Run as a plugin's main, in place of dist/native/main.js, by the native app's tests and screenshots.
const ago = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString();
const url = "https://github.com/acme/web/pull/482";
const args = { provider: "github", url, number: 482, repo: "acme/web", title: "Add retry with backoff to the sync client", cwd: "__ROOT__" };
const status = {
  key: "abc1234", title: args.title, label: "Open", tone: "success", icon: "pr-open", changes: "+212 -48 in 9 files",
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
const run = {
  url: "https://github.com/acme/web/actions/runs/1",
  jobs: [{ id: "1", name: "Tests", stage: null, state: "failed", seconds: 214, steps: [
    { id: "1", name: "Install dependencies", state: "passed", seconds: 41, log: "1" },
    { id: "2", name: "Unit tests", state: "failed", seconds: 182, log: "2" },
    { id: "3", name: "Publish coverage", state: "skipped", seconds: null, log: null },
  ] }],
};
const log = [
  "2026-11-06T09:00:01.000Z ##[group]Run pnpm test",
  "2026-11-06T09:00:01.100Z pnpm test sync",
  "2026-11-06T09:00:01.200Z ##[endgroup]",
  "2026-11-06T09:00:02.000Z \u001b[32m✓\u001b[0m src/sync/module-1.test.ts (8 tests) \u001b[33m120ms\u001b[0m",
  "2026-11-06T09:00:03.000Z \u001b[31m✗\u001b[0m src/sync/client.test.ts (14 tests | 3 failed)",
  "2026-11-06T09:00:03.100Z ##[error]Process completed with exit code 1.",
];
const asked = { "pr-run": () => run, "pr-run-log": () => log };
const respond = (message) => wings.broadcast({ "wings:sdk": 1, ...message });
wings.onBroadcast((m) => {
  if (m?.["wings:sdk"] !== 1) return;
  if (m.kind === "ask") void respond({ kind: "answer", id: m.id, response: asked[m.name]?.() ?? null });
  if (m.kind === "sub") for (const [name, key] of m.subs) void respond({ kind: "state", name, key, data: data[name], at: Date.now() });
});
// A page that subscribed before this listener existed lost its sub. "up" asks every page to subscribe again, as the
// SDK's main does once it serves.
void respond({ kind: "up", boot: "fixture" });
