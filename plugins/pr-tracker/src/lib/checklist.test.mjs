import assert from "node:assert/strict";
import test from "node:test";
import { azureItems, bbItem, between, durationText, ghItem, ghState, glItem, groupChecks, gtItem, hasLog, logTail, withUnresolved } from "./checklist.ts";

const item = (name, state, extra = {}) => ({ id: name, name, state, required: null, kind: null, at: null, seconds: null, url: null, note: null, ...extra });

test("groups run failed, running and queued, passed, then optional and not run, and skip empty ones", () => {
  const groups = groupChecks([
    item("lint", "passed"),
    item("deploy preview", "skipped"),
    item("e2e", "queued"),
    item("unit", "failed"),
    item("build", "running"),
    item("coverage", "passed", { required: false }),
  ]);
  assert.deepEqual(groups.map((g) => [g.id, g.items.map((c) => c.name)]), [
    ["failed", ["unit"]],
    ["active", ["build", "e2e"]],
    ["passed", ["lint"]],
    ["other", ["coverage", "deploy preview"]],
  ]);
  assert.deepEqual(groupChecks([item("a", "passed")]).map((g) => g.id), ["passed"]);
  assert.deepEqual(groupChecks([]), []);
});

test("an optional failure still shows under Failed, and required checks come before optional ones", () => {
  const groups = groupChecks([item("b", "failed", { required: false }), item("a", "failed", { required: true }), item("c", "failed")]);
  assert.deepEqual(groups[0].items.map((c) => c.name), ["a", "b", "c"]);
});

test("durations read as seconds, minutes or hours, and bad times give nothing", () => {
  assert.equal(durationText(45), "45s");
  assert.equal(durationText(134.4), "2m 14s");
  assert.equal(durationText(3900), "1h 05m");
  assert.equal(durationText(null), null);
  assert.equal(durationText(-1), null);
  assert.equal(between("2026-01-01T00:00:00Z", "2026-01-01T00:02:14Z"), 134);
  assert.equal(between("0001-01-01T00:00:00Z", "2026-01-01T00:02:14Z"), null);
  assert.equal(between("2026-01-02T00:00:00Z", "2026-01-01T00:00:00Z"), null);
});

test("GitHub check runs and status contexts map to a state", () => {
  assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion: "SUCCESS" }), "passed");
  assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion: "NEUTRAL" }), "passed");
  assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion: "SKIPPED" }), "skipped");
  for (const conclusion of ["FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"]) assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion }), "failed", conclusion);
  assert.equal(ghState({ __typename: "CheckRun", status: "IN_PROGRESS" }), "running");
  for (const status of ["QUEUED", "WAITING", "PENDING", "REQUESTED"]) assert.equal(ghState({ __typename: "CheckRun", status }), "queued", status);
  assert.equal(ghState({ __typename: "StatusContext", state: "SUCCESS" }), "passed");
  assert.equal(ghState({ __typename: "StatusContext", state: "ERROR" }), "failed");
  assert.equal(ghState({ __typename: "StatusContext", state: "PENDING" }), "running");
  assert.equal(ghState({ __typename: "StatusContext", state: "EXPECTED" }), "queued");
});

test("a failed Actions job on the PR's host can be read, another app's check only links out", () => {
  const job = ghItem({ __typename: "CheckRun", name: "unit", workflowName: "CI", status: "COMPLETED", conclusion: "FAILURE", detailsUrl: "https://github.com/o/r/actions/runs/9/job/77", startedAt: "2026-01-01T00:00:00Z", completedAt: "2026-01-01T00:01:30Z" }, "github.com");
  assert.equal(job.failing.job, "77");
  assert.equal(hasLog(job.failing), true);
  assert.equal(job.kind, "CI");
  assert.equal(job.seconds, 90);
  assert.equal(job.at, "2026-01-01T00:01:30Z");
  const app = ghItem({ __typename: "CheckRun", name: "deploy", status: "COMPLETED", conclusion: "TIMED_OUT", detailsUrl: "https://ci.example.com/b/1" }, "github.com");
  assert.equal(hasLog(app.failing), false);
  assert.equal(app.note, "Timed out");
  const ctx = ghItem({ __typename: "StatusContext", context: "legacy/ci", state: "SUCCESS", targetUrl: "https://ci.example.com/2", startedAt: "0001-01-01T00:00:00Z" }, "github.com");
  assert.equal(ctx.kind, "Status check");
  assert.equal(ctx.at, null);
  assert.equal(ctx.failing, undefined);
});

test("GitLab jobs: manual ones are not run, allowed failures don't block, canceled ones failed", () => {
  const job = (status, extra = {}) => glItem({ id: 5, name: "test", status, stage: "verify", web_url: "https://gitlab.example.com/j/5", duration: 61.2, ...extra }, 3);
  assert.equal(job("success").state, "passed");
  assert.equal(job("manual").state, "skipped");
  assert.equal(job("skipped").state, "skipped");
  assert.equal(job("running").state, "running");
  for (const s of ["created", "pending", "scheduled", "waiting_for_resource"]) assert.equal(job(s).state, "queued", s);
  const allowed = job("failed", { allow_failure: true });
  assert.deepEqual([allowed.state, allowed.required, allowed.failing], ["passed", false, undefined]);
  const failed = job("failed");
  assert.deepEqual([failed.state, failed.required, failed.kind, failed.failing.project, failed.seconds], ["failed", true, "Job in verify", 3, 61.2]);
  assert.equal(job("canceled").state, "failed");
  assert.equal(job("canceled").note, "Canceled");
});

test("Bitbucket and Gitea statuses", () => {
  assert.equal(bbItem({ key: "k", type: "build", state: "SUCCESSFUL", name: "Build", updated_on: "2026-01-01T00:00:00Z" }).state, "passed");
  assert.equal(bbItem({ key: "k", state: "INPROGRESS" }).state, "running");
  const stopped = bbItem({ key: "k", state: "STOPPED" });
  assert.deepEqual([stopped.state, stopped.note, stopped.kind], ["failed", "Stopped", "Status check"]);
  assert.equal(bbItem({ key: "k", type: "build", state: "FAILED" }).kind, "Build");
  assert.deepEqual(["success", "warning", "skipped", "pending", "error", "failure"].map((status) => gtItem({ status, context: "c" }).state), ["passed", "passed", "skipped", "running", "failed", "failed"]);
});

const org = "https://dev.azure.com/acme";
const policy = (type, status, extra = {}) => ({ status, configuration: { isEnabled: true, isBlocking: true, type: { id: "t", displayName: type }, settings: {}, ...extra.configuration }, ...extra.rest });

test("Azure policies: required reviewers say who is missing, a build links to its run", () => {
  const pr = { reviewers: [{ vote: 10, displayName: "Ada", isRequired: true }, { vote: 0, displayName: "Grace", isRequired: true }, { vote: 5, displayName: "Alan" }] };
  const items = azureItems(
    [
      policy("Required reviewers", "queued"),
      policy("Build", "rejected", { configuration: { settings: { displayName: "CI" } }, rest: { context: { buildId: 42 }, startedDate: "2026-01-01T00:00:00Z", completedDate: "2026-01-01T00:03:00Z" } }),
      policy("Work item linking", "rejected"),
      policy("Comment requirements", "rejected"),
      policy("Minimum number of reviewers", "queued", { configuration: { settings: { minimumApproverCount: 2 } } }),
      policy("Status", "notApplicable", { configuration: { isBlocking: false } }),
      policy("Build", "running", { configuration: { isEnabled: false } }),
    ],
    pr,
    org,
    "My Project",
  );
  const by = (kind) => items.find((c) => c.kind === kind);
  assert.equal(items.length, 6, "a disabled policy is not a check");
  assert.equal(by("Required reviewers").note, "1 of 2 required reviewers approved. Waiting for Grace");
  assert.equal(by("Minimum reviewers").note, "2 of 2 approvals");
  const build = by("Build");
  assert.deepEqual([build.name, build.state, build.required, build.seconds, build.url, build.failing], ["CI", "failed", true, 180, `${org}/My%20Project/_build/results?buildId=42`, { id: 42, name: "CI", build: 42 }]);
  assert.equal(by("Work item linking").state, "failed");
  assert.equal(by("Work item linking").failing, undefined);
  assert.equal(by("Comment requirements").needs, "unresolved");
  const na = by("Status check");
  assert.deepEqual([na.state, na.required], ["skipped", false]);
});

test("an unknown Azure policy keeps the name Azure gives it", () => {
  const [c] = azureItems([policy("File size restriction", "approved")], {}, org, "p");
  assert.deepEqual([c.name, c.kind, c.state], ["File size restriction", "File size restriction", "passed"]);
});

test("the merge strategy policy waits instead of failing", () => {
  const [c] = azureItems([policy("Require a merge strategy", "rejected")], {}, org, "p");
  assert.deepEqual([c.state, c.note], ["queued", "Pick the merge type when completing"]);
});

test("a comment requirement counts the open threads once they are known", () => {
  const [c] = azureItems([policy("Comment requirements", "rejected")], {}, org, "p");
  assert.equal(withUnresolved([c], null)[0].note, c.note);
  assert.equal(withUnresolved([c], 3)[0].note, "3 comment threads unresolved");
  assert.equal(withUnresolved([c], 1)[0].note, "1 comment thread unresolved");
  assert.equal(withUnresolved([c], 0)[0].note, "All comment threads resolved");
  assert.equal(withUnresolved([item("x", "passed", { note: "n" })], 5)[0].note, "n");
});

test("a log shows its last lines and says how many came before", () => {
  const log = Array.from({ length: 100 }, (_, i) => `line ${i}`).join("\n");
  const { text, hidden } = logTail(log);
  assert.equal(hidden, 60);
  assert.equal(text.split("\n").length, 40);
  assert.equal(text.split("\n").at(-1), "line 99");
  assert.deepEqual(logTail("a\nb"), { text: "a\nb", hidden: 0 });
});

test("two GitHub contexts that share a link and even a name get different ids", () => {
  const ctx = (context, i) => ghItem({ __typename: "StatusContext", context, state: "SUCCESS", targetUrl: "https://codecov.example/pr/1" }, "github.com", i);
  assert.notEqual(ctx("codecov/patch", 0).id, ctx("codecov/project", 1).id);
  assert.notEqual(ctx("codecov/patch", 0).id, ctx("codecov/patch", 1).id);
});
