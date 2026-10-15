import assert from "node:assert/strict";
import test from "node:test";
import { channel } from "wings-plugin-sdk/messages";

// The badge module reads `wings` when it loads, so a fake goes up first. It isn't focused, so nothing polls.
const ORG = "https://dev.azure.com/acme";
const execs = [];
const badges = new Map();
const is = { signedOut: false, loginFails: false, updateRejects: false, shownWhileSigningIn: "" };
let onPanes;
let onAction;
let toMain;

const git = { "/a": "feature/1111-a", "/b": "feature/2222-b" };
const ok = (v) => ({ code: 0, stdout: typeof v === "string" ? v : JSON.stringify(v), stderr: "" });
globalThis.wings = {
  focused: false,
  onFocus: () => () => {},
  onPanes: (fn) => (onPanes = fn),
  onAction: (fn) => (onAction = fn),
  onBroadcast: (fn) => (toMain = fn),
  setBadge: async (paneId, badge) => void badges.set(paneId, badge),
  installTool: async () => true,
  async exec(program, args, { cwd } = {}) {
    execs.push([program, ...args]);
    if (program === "git") return ok(args[0] === "remote" ? "git@ssh.dev.azure.com:v3/acme/Web/web" : git[cwd]);
    if (args[0] === "login") {
      is.shownWhileSigningIn = badges.get("a").label;
      return is.loginFails ? { code: 1, stdout: "", stderr: "ERROR: Login cancelled\n" } : ok("");
    }
    if (is.signedOut) return { code: 1, stdout: "", stderr: "ERROR: Please run 'az login' to setup account.\n" };
    if (args[2] === "update" && is.updateRejects) throw new Error("az timed out after 30 s");
    if (args[2] === "update") return ok("{}");
    if (args[2] === "show") return ok({ id: Number(args[4]), fields: { "System.Title": "T", "System.TeamProject": "Web", "System.WorkItemType": "Bug", "System.State": "Active" } });
    return ok({ value: [{ name: "Active", category: "InProgress" }, { name: "Resolved", category: "Resolved" }] });
  },
};
const settle = async () => {
  for (let i = 0; i < 4; i++) await new Promise((r) => setImmediate(r));
};
const shows = () => execs.filter((e) => e[3] === "show").map((e) => e[5]);

const { showBadges } = await import("./badge.ts");
showBadges();
onPanes([{ paneId: "a", cwd: "/a" }, { paneId: "b", cwd: "/b" }]);
await settle();

test("item-changed from the item tab refreshes that work item and no other", async () => {
  const tab = channel({ wings: { broadcast: async (m) => toMain(m), onBroadcast: () => () => {} } });
  execs.length = 0;
  await tab.tell("item-changed", { org: ORG, id: 2222 });
  await settle();
  assert.deepEqual(shows(), ["2222"]);
});

test("a failed sign-in puts the Sign in button back instead of leaving Signing in…", async () => {
  is.signedOut = true;
  await onAction({ paneId: "a", actionId: "refresh" });
  assert.equal(badges.get("a").label, "Sign in to Azure DevOps");
  is.loginFails = true;
  await assert.rejects(onAction({ paneId: "a", actionId: "signin" }), /Login cancelled/);
  assert.equal(is.shownWhileSigningIn, "Signing in…");
  assert.equal(badges.get("a").label, "Sign in to Azure DevOps");
  assert.equal(badges.get("a").loading, false);
  is.signedOut = false;
  is.loginFails = false;
  await onAction({ paneId: "a", actionId: "retry" });
  await settle();
  assert.equal(badges.get("a").label, "#1111 Active");
});

test("a move whose az call can't run stops the spinner", async () => {
  is.updateRejects = true;
  await assert.rejects(onAction({ paneId: "a", actionId: "move:Resolved" }), /timed out/);
  assert.equal(badges.get("a").loading, false);
});
