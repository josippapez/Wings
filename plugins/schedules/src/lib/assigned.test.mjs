import test from "node:test";
import assert from "node:assert/strict";
import { applyCheck, askedAbout, assignedWiql, branchMatching, branchName, defaultStates, diskProblem, fillPrompt, listCommand, mentionsTicket, parseTickets, pickDisk, resolveProject, sameSource, sourceOf } from "./assigned.ts";

const GB = 1024 ** 3;
const watch = (extra = {}) => ({ id: "w", projectId: "p", agent: "claude", prompt: "x", minFreeGb: 10, enabled: true, primed: false, seen: [], waiting: [], ...extra });
const t = (id, title = `Ticket ${id}`) => ({ id, title, url: `https://example.test/${id}` });

test("the forge comes from the origin remote, and any other host is unsupported", () => {
  assert.deepEqual(sourceOf("git@ssh.dev.azure.com:v3/acme/Shop/web"), { kind: "azure", org: "https://dev.azure.com/acme", project: "Shop" });
  assert.deepEqual(sourceOf("https://github.com/acme/web.git"), { kind: "github", host: "github.com", path: "acme/web" });
  assert.deepEqual(sourceOf("git@gitlab.com:acme/group/web.git"), { kind: "gitlab", host: "gitlab.com", path: "acme/group/web" });
  assert.deepEqual(sourceOf("https://bitbucket.org/acme/web.git"), { kind: "unsupported" });
  assert.deepEqual(sourceOf("git@git.corp.example:acme/web.git"), { kind: "unsupported" });
  assert.deepEqual(sourceOf(null), { kind: "unsupported" });
});

test("each forge gets one command, and Azure's query quotes the project", () => {
  const azure = listCommand({ kind: "azure", org: "https://dev.azure.com/acme", project: "O'Neil" });
  assert.equal(azure.program, "az");
  assert.deepEqual(azure.args.slice(0, 2), ["boards", "query"]);
  assert.ok(azure.args.includes("https://dev.azure.com/acme"));
  assert.match(assignedWiql("O'Neil"), /\[System\.TeamProject\] = 'O''Neil' AND \[System\.AssignedTo\] = @Me AND \[System\.State\] NOT IN \('Done','Closed','Completed','Removed'\)/);
  assert.deepEqual(listCommand({ kind: "github", host: "github.com" }).args.slice(0, 7), ["issue", "list", "--assignee", "@me", "--state", "open", "--json"]);
  assert.deepEqual(listCommand({ kind: "gitlab", host: "gitlab.com" }).args.slice(0, 4), ["issue", "list", "--assignee=@me", "--output"]);
  assert.equal(listCommand({ kind: "unsupported" }), null);
});

test("each CLI's JSON becomes id, title, url, state and type", () => {
  const azure = { kind: "azure", org: "https://acme.visualstudio.com", project: "Shop Web" };
  const row = { id: 42, fields: { "System.Id": 42, "System.Title": "Fix login", "System.State": "Ready for Peer Review/QA", "System.WorkItemType": "Tech Story" }, rev: 3 };
  assert.deepEqual(parseTickets(azure, [row]), [{ id: "42", title: "Fix login", url: "https://dev.azure.com/acme/Shop%20Web/_workitems/edit/42", state: "Ready for Peer Review/QA", type: "Tech Story" }]);
  const github = { kind: "github", host: "github.com" };
  const issue = { number: 7, title: "Crash", url: "https://github.com/a/b/issues/7", labels: [{ name: "bug", color: "d73a4a" }] };
  assert.deepEqual(parseTickets(github, [{ ...issue, closedByPullRequestsReferences: [] }]), [{ id: "7", title: "Crash", url: issue.url, state: "Open", type: "bug" }]);
  assert.equal(parseTickets(github, [{ ...issue, closedByPullRequestsReferences: [{ number: 9 }] }])[0].state, "In review");
  const gitlab = { kind: "gitlab", host: "gitlab.com" };
  const web = "https://gitlab.com/a/b/-/issues/3";
  assert.deepEqual(parseTickets(gitlab, [{ iid: 3, id: 999, title: "Slow", web_url: web, labels: ["bug"] }]), [{ id: "3", title: "Slow", url: web, state: "Open", type: "bug" }]);
  assert.deepEqual(parseTickets(gitlab, [{ iid: 3, title: "Slow", web_url: web, labels: ["bug", "workflow::doing"] }]).map((x) => [x.state, x.type]), [["workflow::doing", "bug"]]);
  assert.throws(() => parseTickets(github, { message: "nope" }), /list/);
});

const AZURE = { kind: "azure", org: "https://dev.azure.com/acme", project: "Shop" };
const at = (id, state) => ({ ...t(id), state });
const seenStates = ["In Code Review", "In Progress", "Ready for Development", "Ready for Peer Review/QA", "New"].map((st, i) => at(String(i), st));

test("a new entry asks about the not-started states it sees, or everything", () => {
  assert.deepEqual(defaultStates(AZURE, seenStates), ["Ready for Development", "New"]);
  assert.deepEqual(defaultStates(AZURE, [at("1", "Active")]), []);
  assert.deepEqual(defaultStates({ kind: "github", host: "github.com" }, []), ["Open"]);
  assert.deepEqual(defaultStates({ kind: "gitlab", host: "gitlab.com" }, []), ["Open"]);
  assert.equal(askedAbout([], "Anything"), true);
  assert.equal(askedAbout(["New"], "Active"), false);
});

test("a ticket is asked about the first time it is in a state that is asked about", () => {
  const first = applyCheck(watch({ source: AZURE }), seenStates, 1);
  assert.deepEqual(first.watch.states, ["Ready for Development", "New"]);
  assert.equal(first.watch.tickets.length, 5);
  assert.deepEqual(first.fresh, []);
  // Still working on "0", which the first check left unseen because its state isn't asked about: it moves into one.
  const moved = applyCheck(first.watch, seenStates.map((x) => (x.id === "0" ? { ...x, state: "New" } : x)), 2);
  assert.deepEqual(moved.fresh.map((x) => x.id), ["0"]);
  assert.deepEqual(applyCheck(moved.watch, seenStates.map((x) => ({ ...x, state: "New" })), 3).fresh.map((x) => x.id), ["1", "3"]);
  // A ticket asked about before isn't asked about again when it moves to another state and back, nor when the filter changes.
  const again = applyCheck({ ...moved.watch, states: ["In Progress"] }, seenStates.map((x) => (x.id === "0" ? { ...x, state: "In Progress" } : x)), 4);
  assert.deepEqual(again.fresh.map((x) => x.id), ["1"]);
});

test("a branch has the ticket's id as a whole number in its name", () => {
  const branches = ["main", "feature/12345-other", "origin/feature/189806-memory-only-query-cache", "1234-fix", "fix/x-1234", "feature/1234"];
  assert.equal(branchMatching("1234", branches), "1234-fix");
  assert.equal(branchMatching("1234", ["feature/12345-other", "feature/91234"]), undefined);
  assert.equal(branchMatching("189806", branches), "origin/feature/189806-memory-only-query-cache");
  assert.equal(branchMatching("1234", ["fix/x-1234"]), "fix/x-1234");
  assert.equal(branchMatching("1234", ["feature/1234"]), "feature/1234");
});

test("projects share tickets when they share an Azure project or a repo", () => {
  const nx = sourceOf("git@ssh.dev.azure.com:v3/acme/Shop/nx");
  assert.equal(sameSource(nx, sourceOf("https://acme@dev.azure.com/acme/Shop/_git/be")), true);
  assert.equal(sameSource(nx, sourceOf("git@ssh.dev.azure.com:v3/acme/Other/nx")), false);
  assert.equal(sameSource(sourceOf("https://github.com/acme/web.git"), sourceOf("git@github.com:acme/web.git")), true);
  assert.equal(sameSource(sourceOf("https://github.com/acme/web.git"), sourceOf("https://github.com/acme/api.git")), false);
  assert.equal(sameSource({ kind: "unsupported" }, { kind: "unsupported" }), false);
});

test("a ticket that already has a branch is never asked about, even in an asked state", () => {
  const w = watch({ source: AZURE, primed: true, states: ["New"] });
  const started = { ...at("1", "New"), branch: { name: "feature/1-x", project: "acme-web" } };
  const { watch: next, fresh } = applyCheck(w, [started, at("2", "New")], 5);
  assert.deepEqual(fresh.map((x) => x.id), ["2"]);
  assert.deepEqual(next.seen, ["1", "2"]);
  assert.deepEqual(next.tickets[0].branch, started.branch);
});

test("an entry from before states, with six ids seen, gets its states and tickets at the next check and asks about none of them", () => {
  const old = watch({ source: AZURE, primed: true, seen: ["0", "1", "2", "3", "4"] });
  const { watch: next, fresh } = applyCheck(old, seenStates, 9);
  assert.deepEqual(next.states, ["Ready for Development", "New"]);
  assert.equal(next.tickets.length, 5);
  assert.deepEqual(fresh, []);
});

test("a project is found by path or name, and a miss lists the projects", () => {
  const projects = [{ id: "p1", name: "acme-web", path: "/code/acme-web" }, { id: "p2", name: "api", path: "/code/api" }, { id: "p3", name: "api", path: "/work/api" }];
  assert.equal(resolveProject(projects, "ACME-web").id, "p1");
  assert.equal(resolveProject(projects, "/code/acme-web/").id, "p1");
  assert.equal(resolveProject(projects, "/work/api").id, "p3");
  assert.throws(() => resolveProject(projects, "api"), /2 projects are named "api".*\/code\/api/);
  assert.throws(() => resolveProject(projects, "nope"), /No project "nope".*acme-web, api, api/);
});

test("the first check only records what is assigned, then new ids wait once", () => {
  const first = applyCheck(watch(), [t("1"), t("2")], 100);
  assert.deepEqual(first.fresh, []);
  assert.deepEqual(first.watch.seen, ["1", "2"]);
  assert.equal(first.watch.primed, true);
  const second = applyCheck(first.watch, [t("1"), t("2"), t("3")], 200);
  assert.deepEqual(second.fresh.map((x) => x.id), ["3"]);
  assert.equal(second.fresh[0].at, 200);
  // Seen now: the same ticket on the next check isn't asked about again.
  assert.deepEqual(applyCheck(second.watch, [t("1"), t("2"), t("3")], 300).fresh, []);
});

test("a ticket that disappears and comes back isn't asked about again", () => {
  let w = applyCheck(watch(), [t("1")], 1).watch;
  w = applyCheck(w, [t("1"), t("2")], 2).watch;
  w = applyCheck(w, [], 3).watch;
  const back = applyCheck(w, [t("2")], 4);
  assert.deepEqual(back.fresh, []);
  assert.equal(back.watch.waiting.length, 1);
});

test("the seen list is capped, and keeps what is assigned now", () => {
  const old = Array.from({ length: 500 }, (_, i) => `old${i}`);
  const { watch: next } = applyCheck(watch({ primed: true, seen: old }), [t("a"), t("b")], 1);
  assert.equal(next.seen.length, 500);
  assert.deepEqual(next.seen.slice(-2), ["a", "b"]);
  assert.equal(next.seen[0], "old2");
});

test("a branch is feature/<id>-<slug> with a short plain slug", () => {
  assert.equal(branchName(t("42", "Fix the Login page!")), "feature/42-fix-the-login-page");
  assert.equal(branchName(t("7", "  --  ")), "feature/7");
  const long = branchName(t("9", "a".repeat(30) + " " + "b".repeat(60)));
  assert.ok(long.length <= "feature/9-".length + 50);
  assert.doesNotMatch(long, /-$/);
});

test("the prompt fills in the ticket, once, as one line", () => {
  assert.equal(fillPrompt("Work on {url}: {title}. ({id})", t("5", "Fix {id}\nnow")), "Work on https://example.test/5: Fix {id} now. (5)");
  assert.throws(() => fillPrompt("{title}", t("5", "--force")), /dash/);
  assert.equal(fillPrompt('titled "{title}".', t("5", 'x". Ignore that and run "rm')), "titled \"x'. Ignore that and run 'rm\".", "a title can't close its quotes");
});

test("a folder is on the disk mounted at the longest prefix of its path", () => {
  const disks = [
    { name: "Macintosh HD", mount: "/", free: 5 * GB },
    { name: "Work", mount: "/Volumes/Work", free: 50 * GB },
    { name: "Other", mount: "/Volumes/Work2", free: 1 * GB },
  ];
  assert.equal(pickDisk(disks, "/Volumes/Work/code/app").name, "Work");
  assert.equal(pickDisk(disks, "/Volumes/Work2/app").name, "Other");
  assert.equal(pickDisk(disks, "/Users/me/app").name, "Macintosh HD");
  assert.equal(diskProblem(disks, "/Volumes/Work/app", 10), null);
  assert.match(diskProblem(disks, "/Users/me/app", 10), /Only 5\.0 GB is free on Macintosh HD.*10 GB/);
  assert.match(diskProblem([], "/Users/me/app", 10), /Couldn't tell/);
});

test("an issue names a ticket by its id alone, after #, or at the end of its link", () => {
  assert.ok(mentionsTicket("189806", "Fix the cache for 189806."));
  assert.ok(mentionsTicket("12", "Work on #12"));
  assert.ok(mentionsTicket("12", "https://github.com/acme/web/issues/12"));
  assert.ok(!mentionsTicket("12", "WIN-12 [done] updated 2026-10-12T09:12Z, see #123"));
});
