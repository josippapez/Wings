import assert from "node:assert/strict";
import { test } from "node:test";
import { setImmediate as next } from "node:timers/promises";

import { cli, CliError } from "./cli.js";

const done = (stdout = "") => ({ code: 0, stdout, stderr: "" });
const fail = (stderr, code = 1) => ({ code, stdout: "", stderr });

/** A `wings` whose `exec` answers from `handler(program, args)` (a result, an Error to reject with, or a promise of one). */
function fakeWings(handler) {
  const calls = [];
  const exec = async (program, args, options) => {
    calls.push({ program, args, ...options });
    const reply = await handler(program, args);
    if (reply instanceof Error) throw reply;
    return reply;
  };
  return { calls, exec, installTool: async (program) => (calls.push({ install: program }), true) };
}

const AZ = { signedOut: /login/i, signIn: ["login", "--allow-no-subscriptions", "--output", "none"] };
const TEXT = { title: "Azure DevOps", missing: "Needs az.", signedOut: "Sign in, please." };
const labels = (c, key) => c.problemBadge(key, TEXT)?.label;

test("a program that isn't installed is a missing problem with an Install badge, unless the call is quiet", async () => {
  const w = fakeWings(() => new Error("az is not installed or not on PATH"));
  const az = cli("az", { ...AZ, wings: w });
  let changes = 0;
  az.onChange(() => changes++);
  await assert.rejects(az.run(["boards"], { key: "org", quiet: true }), { name: "CliError", kind: "missing", message: "az is not installed or not on PATH" });
  assert.equal(az.problem("org"), null);
  await assert.rejects(az.json(["boards"], { key: "org" }), { kind: "missing" });
  assert.deepEqual(az.problem("org"), { kind: "missing", message: "az is not installed or not on PATH" });
  const actions = [{ id: "install", label: "Install", primary: true }, { id: "retry", label: "Try again" }];
  assert.deepEqual(az.problemBadge("org", TEXT), { tone: "warning", title: "Azure DevOps", label: "Install Azure CLI", subtitle: "Needs az.", actions });
  assert.equal(az.problem("other"), null);
  assert.equal(changes, 1);
  assert.equal(await az.install(), true);
  assert.deepEqual(w.calls.at(-1), { install: "az" });
});

test("a timeout or a refused call is a plain failure with its own message", async () => {
  const az = cli("az", { ...AZ, wings: fakeWings(() => new Error("az timed out after 30s")) });
  await assert.rejects(az.run(["boards"], { key: "org" }), { kind: "failed", message: "az timed out after 30s" });
  assert.equal(az.problem("org"), null);
});

test("a signed-out program is a problem under its key until a call works, and a plain failure changes nothing", async () => {
  let reply = fail("WARNING: something\nERROR: Please run 'az login' to setup account.\n");
  const az = cli("az", { ...AZ, wings: fakeWings(() => reply) });
  await assert.rejects(az.run(["boards"], { key: "org", quiet: true }), { kind: "signed-out" });
  assert.equal(az.problem("org"), null, "a quiet call records nothing");
  await assert.rejects(az.run(["boards"], { key: "org" }), { kind: "signed-out", message: "ERROR: Please run 'az login' to setup account.", stderr: reply.stderr });
  assert.deepEqual([labels(az, "org"), labels(az, "other")], ["Sign in to Azure DevOps", undefined]);
  assert.deepEqual([az.problemBadge("org", TEXT).subtitle, az.problemBadge("org", TEXT).loading], ["Sign in, please.", false]);

  reply = fail("ERROR: TF401232: Work item 9 does not exist.\n");
  for (const key of ["org", "other"]) {
    await assert.rejects(az.run(["show"], { key }), (e) => e instanceof CliError && e.kind === "failed" && e.message === "ERROR: TF401232: Work item 9 does not exist.");
  }
  assert.deepEqual([az.problem("org").kind, az.problem("other")], ["signed-out", null]);
  reply = done("{}");
  await az.run(["show"], { key: "org", quiet: true });
  assert.equal(az.problem("org").kind, "signed-out", "a quiet success clears nothing either");
  await az.run(["show"], { key: "org" });
  assert.equal(az.problem("org"), null);
});

test("json parses what the program printed, and gives null for nothing", async () => {
  const az = cli("az", { ...AZ, wings: fakeWings((p, args) => done(args[0] === "empty" ? "\n" : '{"id":7}')) });
  assert.deepEqual([await az.json(["show"]), await az.json(["empty"])], [{ id: 7 }, null]);
});

test("signedOut can be a function for a signal that takes a second command to see, and sticky keeps the problem", async () => {
  const w = fakeWings((p, args) => (args[0] === "auth" ? fail("not logged in") : args[1] === "public" ? done("[]") : fail("glab: 404 Not Found")));
  const signedOut = async (r, key) => /\b404\b/.test(r.stderr) && (await w.exec("glab", ["auth", "status", "--hostname", key])).code !== 0;
  const glab = cli("glab", { sticky: true, signedOut, signIn: (key) => ["auth", "login", "--web", "--hostname", key], wings: w });
  await assert.rejects(glab.run(["api", "private"], { key: "gitlab.example.com" }), { kind: "signed-out" });
  assert.deepEqual(w.calls.at(-1).args, ["auth", "status", "--hostname", "gitlab.example.com"]);
  await glab.run(["api", "public"], { key: "gitlab.example.com" });
  assert.equal(glab.problem("gitlab.example.com").kind, "signed-out");
});

test("signIn runs the login, shows Signing in… meanwhile, then retries: problems go and the listeners run", async () => {
  let finish;
  const w = fakeWings((p, args) => (args[0] === "login" ? new Promise((resolve) => (finish = resolve)) : fail("ERROR: Please run 'az login'")));
  const az = cli("az", { ...AZ, wings: w });
  await assert.rejects(az.run(["boards"], { key: "org" }), { kind: "signed-out" });
  let reloads = 0;
  const seen = [];
  az.onRetry(async () => {
    await next();
    reloads++;
  });
  az.onChange(() => seen.push(labels(az, "org")));

  const lines = [];
  const signing = az.signIn(undefined, { onOutput: (line) => lines.push(line) });
  const login = w.calls.at(-1);
  assert.deepEqual([login.program, login.args, login.timeoutMs], ["az", AZ.signIn, 300_000]);
  login.onOutput("code");
  assert.deepEqual([lines, az.problemBadge("org", TEXT).loading], [["code"], true]);
  const calls = w.calls.length;
  assert.equal(await az.signIn(), undefined);
  assert.equal(w.calls.length, calls, "a second sign-in does nothing");

  finish(done());
  await signing;
  assert.deepEqual([reloads, az.problem("org"), seen], [1, null, ["Signing in…", undefined]]);
});

test("a sign-in that fails throws the program's message, and the badge offers Sign in again", async () => {
  const w = fakeWings((p, args) => (args[0] === "login" ? fail("ERROR: Authentication cancelled\n") : fail("Please run 'az login'")));
  const az = cli("az", { ...AZ, signIn: (key) => ["login", key], wings: w });
  await assert.rejects(az.run(["boards"], { key: "org" }), { kind: "signed-out" });
  let reloads = 0;
  const seen = [];
  az.onRetry(() => reloads++);
  az.onChange(() => seen.push(labels(az, "org")));
  await assert.rejects(az.signIn("org"), { message: "ERROR: Authentication cancelled" });
  assert.deepEqual(w.calls.at(-1).args, ["login", "org"]);
  assert.deepEqual([seen, az.problem("org").kind, reloads], [["Signing in…", "Sign in to Azure DevOps"], "signed-out", 0]);
});

test("a helper made before wings exists uses the wings there is when it runs", async (t) => {
  const before = globalThis.wings;
  t.after(() => (globalThis.wings = before));
  delete globalThis.wings;
  const az = cli("az", AZ);
  globalThis.wings = fakeWings(() => done("ok"));
  assert.equal(await az.run(["version"]), "ok");
});
