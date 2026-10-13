import assert from "node:assert/strict";
import { test } from "node:test";

import { backoffMs, dayBefore, diffPrs, fresh, mentionsMe, notes, plain, remember, rememberProject, snapshot, statusEvent, threadEvents, watermark, webBase, workItemEvents } from "./updates.ts";
import { createWatcher } from "./watch.ts";

const ME = "11111111-1111-4111-8111-111111111111";
const ANA = "22222222-2222-4222-8222-222222222222";
const BOB = "33333333-3333-4333-8333-333333333333";
const ORG = "https://dev.azure.com/example";
const PROJECT = "Example Project";
const SINCE = "2030-01-01T10:00:00.000Z";
const LATER = "2030-01-01T10:05:00.000Z";
const EARLIER = "2030-01-01T09:00:00.000Z";

const mention = (id, name = "Sam Example") => `<div><a href="#" data-vss-mention="version:2.0,${id}">@${name}</a> can you look?</div>`;

test("a work item mention is recognised by the exact identity id, in either case", () => {
  assert.equal(mentionsMe(mention(ME), ME), true);
  assert.equal(mentionsMe(mention(ME.toUpperCase()), ME), true);
  assert.equal(mentionsMe(mention(ANA), ME), false);
  // The same id typed as plain text, or only partly matching, is not a mention.
  assert.equal(mentionsMe(`${ME} without markup`, ME), false);
  assert.equal(mentionsMe(mention(ME.slice(0, 35) + "2"), ME), false);
  assert.equal(mentionsMe(`${mention(ANA)} ${mention(ME)}`, ME), true);
  assert.equal(mentionsMe(undefined, ME), false);
});

test("a pull request mention is @<id>", () => {
  assert.equal(mentionsMe(`@<${ME}> please check`, ME), true);
  assert.equal(mentionsMe(`@<{${ME}}> please check`, ME), true);
  assert.equal(mentionsMe(`@<${ANA}> please check`, ME), false);
  assert.equal(mentionsMe(`email@<${ANA}>`, ME), false);
});

test("plain turns a comment into one line", () => {
  assert.equal(plain(`<div>Hi &amp; <b>bye</b><br>two</div>`), "Hi & bye two");
  assert.equal(plain(`@<${ME}> ok`), "@you ok");
  assert.equal(plain("x".repeat(200), 10).length, 10);
});

const update = (id, at, by, fields) => ({ id, revisedBy: { id: by, displayName: "Ana Example" }, revisedDate: "9999-01-01T00:00:00Z", fields: { "System.ChangedDate": { newValue: at }, ...fields } });
const item = { id: 42, title: "Example task", mine: true };

test("a state change on my item by someone else is an event, with the old and new state", () => {
  const events = workItemEvents(ORG, PROJECT, item, [update(5, LATER, ANA, { "System.State": { oldValue: "Active", newValue: "Resolved" } })], ME, SINCE);
  assert.equal(events.length, 1);
  assert.equal(events[0].kind, "state");
  assert.equal(events[0].title, "#42 Active → Resolved");
  assert.equal(events[0].url, `${ORG}/Example%20Project/_workitems/edit/42`);
});

test("state changes before the watermark, by me, on items that aren't mine, or that keep the state are not events", () => {
  const change = { "System.State": { oldValue: "Active", newValue: "Resolved" } };
  assert.equal(workItemEvents(ORG, PROJECT, item, [update(5, EARLIER, ANA, change)], ME, SINCE).length, 0);
  assert.equal(workItemEvents(ORG, PROJECT, item, [update(5, LATER, ME, change)], ME, SINCE).length, 0);
  assert.equal(workItemEvents(ORG, PROJECT, { ...item, mine: false }, [update(5, LATER, ANA, change)], ME, SINCE).length, 0);
  assert.equal(workItemEvents(ORG, PROJECT, item, [update(5, LATER, ANA, { "System.State": { oldValue: "Active", newValue: "Active" } })], ME, SINCE).length, 0);
});

test("only a comment that mentions me counts, on any item, and not one I wrote", () => {
  const comment = (by, html) => update(6, LATER, by, { "System.History": { newValue: html } });
  const other = { ...item, mine: false };
  assert.equal(workItemEvents(ORG, PROJECT, other, [comment(ANA, mention(ME))], ME, SINCE)[0].kind, "mention");
  assert.equal(workItemEvents(ORG, PROJECT, other, [comment(ANA, mention(BOB))], ME, SINCE).length, 0);
  assert.equal(workItemEvents(ORG, PROJECT, other, [comment(ME, mention(ME))], ME, SINCE).length, 0);
});

test("the newest update is timed by its ChangedDate, since its revisedDate is year 9999", () => {
  const u = { id: 9, revisedBy: { id: ANA }, revisedDate: "9999-01-01T00:00:00Z", fields: { "System.ChangedDate": { newValue: LATER }, "System.State": { newValue: "Done", oldValue: "Active" } } };
  assert.equal(workItemEvents(ORG, PROJECT, item, [u], ME, SINCE).length, 1);
  assert.equal(workItemEvents(ORG, PROJECT, item, [{ ...u, fields: undefined }], ME, SINCE).length, 0);
});

const pr = (over = {}) => ({ pullRequestId: 7, title: "Example change", status: "active", createdBy: { id: ME, displayName: "Sam Example" }, reviewers: [{ id: ANA, displayName: "Ana Example", vote: 0 }], repository: { name: "example-repo", id: "r1" }, ...over });

test("a reviewer's new vote on my pull request is an event; an unchanged vote is not", () => {
  const prev = snapshot([pr()], ME);
  const approved = pr({ reviewers: [{ id: ANA, displayName: "Ana Example", vote: 10 }] });
  const { events } = diffPrs(ORG, PROJECT, prev, [approved], ME, LATER);
  assert.deepEqual(events.map((e) => [e.kind, e.title]), [["pr-vote", "Ana Example approved on PR #7"]]);
  assert.equal(diffPrs(ORG, PROJECT, snapshot([approved], ME), [approved], ME, LATER).events.length, 0);
  assert.equal(diffPrs(ORG, PROJECT, snapshot([approved], ME), [pr({ reviewers: [{ id: ANA, vote: -5 }] })], ME, LATER).events[0].title.includes("waiting for the author"), true);
});

test("being added as a reviewer is an event once; my own new pull request is not", () => {
  const theirs = pr({ createdBy: { id: ANA, displayName: "Ana Example" }, reviewers: [{ id: ME, vote: 0 }] });
  const first = diffPrs(ORG, PROJECT, {}, [theirs], ME, LATER);
  assert.equal(first.events[0].kind, "pr-reviewer");
  assert.equal(diffPrs(ORG, PROJECT, snapshot([theirs], ME), [theirs], ME, LATER).events.length, 0);
  assert.equal(diffPrs(ORG, PROJECT, {}, [pr()], ME, LATER).events.length, 0);
});

test("my pull request leaving the active list is looked up, and completed or abandoned is an event", () => {
  const prev = snapshot([pr(), pr({ pullRequestId: 8, createdBy: { id: ANA } })], ME);
  assert.deepEqual(diffPrs(ORG, PROJECT, prev, [], ME, LATER).gone, [7]);
  assert.equal(statusEvent(ORG, PROJECT, pr({ status: "completed" }), LATER).title, "PR #7 was completed");
  assert.equal(statusEvent(ORG, PROJECT, pr({ status: "abandoned" }), LATER).title, "PR #7 was abandoned");
  assert.equal(statusEvent(ORG, PROJECT, pr({ status: "active" }), LATER), null);
});

const thread = (id, comments, extra = {}) => ({ id, comments, ...extra });
const comment = (id, by, content, over = {}) => ({ id, author: { id: by, displayName: "Ana Example" }, content, commentType: "text", publishedDate: LATER, ...over });

test("on my pull request every new comment by someone else counts; mine, system and old ones don't", () => {
  const threads = [
    thread(1, [comment(1, ANA, "Looks off"), comment(2, ME, "Fixed"), comment(3, ANA, "old", { publishedDate: EARLIER }), comment(4, ANA, "merged", { commentType: "system" })]),
    thread(2, [comment(1, ANA, "deleted", { isDeleted: true })]),
    thread(3, [comment(1, ANA, "gone")], { isDeleted: true }),
  ];
  const events = threadEvents(ORG, PROJECT, pr(), threads, ME, SINCE);
  assert.deepEqual(events.map((e) => [e.kind, e.body]), [["pr-comment", "Looks off"]]);
  assert.equal(events[0].url, `${ORG}/Example%20Project/_git/example-repo/pullrequest/7?_a=overview&discussionId=1`);
});

test("on a pull request I review only mentions and replies in my threads count", () => {
  const theirs = pr({ createdBy: { id: ANA } });
  const threads = [
    thread(1, [comment(1, ANA, "no one in particular")]),
    thread(2, [comment(1, ANA, `@<${ME}> thoughts?`)]),
    thread(3, [comment(1, ME, "my question", { publishedDate: EARLIER }), comment(2, ANA, "an answer")]),
  ];
  assert.deepEqual(threadEvents(ORG, PROJECT, theirs, threads, ME, SINCE).map((e) => [e.kind, e.key.split(":").slice(-2).join(":")]), [["pr-mention", "2:1"], ["pr-comment", "3:2"]]);
});

test("watermark leaves an overlap, and the day to ask WIQL for starts a day earlier", () => {
  assert.equal(watermark(Date.parse(LATER), 120_000), "2030-01-01T10:03:00.000Z");
  assert.equal(dayBefore(SINCE), "2029-12-31");
});

test("events already seen are dropped, once each, and seen keeps only the newest", () => {
  const e = (key, at) => ({ key, kind: "state", title: key, body: "", url: "", at });
  assert.deepEqual(fresh([e("a", LATER), e("b", LATER), e("a", LATER)], { b: LATER }).map((x) => x.key), ["a"]);
  assert.deepEqual(Object.keys(remember({ old: EARLIER }, [e("new", LATER)], 1)), ["new"]);
});

test("up to 3 events notify one by one, more become one summary", () => {
  const events = Array.from({ length: 5 }, (_, i) => ({ key: String(i), kind: "state", title: `Event ${i}`, body: `Body ${i}`, url: "", at: LATER }));
  assert.equal(notes(events.slice(0, 3)).length, 3);
  const summary = notes(events);
  assert.equal(summary.length, 1);
  assert.equal(summary[0].title, "5 Azure DevOps updates");
  assert.match(summary[0].body, /Event 0; Event 1; Event 2; and 2 more/);
  assert.deepEqual(notes([]), []);
});

test("backoff doubles from the interval up to 30 minutes", () => {
  assert.deepEqual([1, 2, 3, 10].map((n) => backoffMs(n, 120_000)), [120_000, 240_000, 480_000, 1_800_000]);
});

test("known projects keep the newest first, without repeats, up to a cap", () => {
  let known = {};
  for (const p of ["a", "b", "a", "c"]) known = rememberProject(known, ORG, p, 2);
  assert.deepEqual(known[ORG], ["c", "a"]);
});

test("visualstudio.com orgs link to dev.azure.com", () => {
  assert.equal(webBase("https://example.visualstudio.com"), "https://dev.azure.com/example");
});

// ---------- a whole check, against a stub az ----------

function harness(responses) {
  const store = new Map();
  const calls = [];
  const notified = [];
  let clock = Date.parse(SINCE);
  const deps = {
    az: async (org, args) => {
      calls.push(args);
      return responses(args);
    },
    get: async (k) => store.get(k) ?? null,
    set: async (k, v) => void store.set(k, structuredClone(v)),
    notify: async (n) => void notified.push(n),
    now: () => clock,
  };
  return { deps, store, calls, notified, advance: (ms) => (clock += ms) };
}

const resource = (args) => args[args.indexOf("--resource") + 1];
const wiqlRow = (id, changed, who = ME) => ({ id, fields: { "System.Title": `Item ${id}`, "System.ChangedDate": changed, "System.AssignedTo": { id: who }, "System.CreatedBy": { id: ANA } } });

test("first check only sets the watermark and the snapshot: nothing is reported, and no work item is read", async () => {
  const h = harness((args) => {
    if (args[1] === "query") return [{ id: 1, fields: { "System.ChangedBy": { id: ME } } }];
    if (resource(args) === "pullRequests") return { value: [pr()] };
    throw new Error(`unexpected ${args.join(" ")}`);
  });
  const w = createWatcher(h.deps);
  assert.deepEqual(await w.cycle([{ org: ORG, project: PROJECT }], 120_000), []);
  assert.equal(h.store.get(`watch:wm:${ORG}|${PROJECT}`), watermark(Date.parse(SINCE)));
  assert.equal(h.store.get(`watch:me:${ORG}`), ME);
  assert.deepEqual(h.store.get(`watch:prs:${ORG}|${PROJECT}`), snapshot([pr()], ME));
  assert.equal(h.notified.length, 0);
});

test("a later check reports state changes, mentions and pull request activity once, and records them", async () => {
  const comments = [comment(1, ANA, "Please rename this")];
  const h = harness((args) => {
    if (args[1] === "query") return args.join(" ").includes("@Me") ? [{ id: 1, fields: { "System.ChangedBy": { id: ME } } }] : [wiqlRow(42, LATER), wiqlRow(43, LATER, BOB), wiqlRow(44, EARLIER)];
    const r = resource(args);
    if (r === "updates") {
      const id = args.find((a) => a.startsWith("id=")).slice(3);
      return { value: id === "42" ? [update(3, LATER, ANA, { "System.State": { oldValue: "Active", newValue: "Resolved" } })] : [update(3, LATER, ANA, { "System.History": { newValue: mention(ME) } })] };
    }
    if (r === "pullRequests") return { value: [pr()] };
    if (r === "pullRequestThreads") return { value: [thread(1, comments)] };
    throw new Error(`unexpected ${args.join(" ")}`);
  });
  const w = createWatcher(h.deps);
  const target = { org: ORG, project: PROJECT };
  await w.cycle([target], 120_000);
  h.advance(120_000);
  const events = await w.cycle([target], 120_000);
  assert.deepEqual(events.map((e) => e.kind).sort(), ["mention", "pr-comment", "state"]);
  assert.equal(h.notified.length, 3);
  assert.equal((await w.stored()).length, 3);
  // Item 44 changed before the watermark, so its updates weren't read.
  assert.equal(h.calls.some((a) => a.includes("id=44")), false);
  // The overlap in the watermark means the same activity is seen again, and reported once.
  h.advance(120_000);
  assert.deepEqual(await w.cycle([target], 120_000), []);
  assert.equal(h.notified.length, 3);
});

test("only read calls: no write flag, no POST, and only the declared az commands", async () => {
  const h = harness((args) => (args[1] === "query" ? [{ id: 1, fields: { "System.ChangedBy": { id: ME } } }] : { value: [] }));
  const w = createWatcher(h.deps);
  await w.cycle([{ org: ORG, project: PROJECT }], 120_000);
  h.advance(120_000);
  await w.cycle([{ org: ORG, project: PROJECT }], 120_000);
  for (const args of h.calls) {
    assert.ok(args[0] === "boards" && args[1] === "query" || args[0] === "devops" && args[1] === "invoke", args.join(" "));
    assert.equal(args.some((a) => /^--(http-method|method|in-file)$/.test(a)), false);
  }
});

test("a failing project is skipped with backoff, its watermark stays, and other projects still run", async () => {
  let fail = true;
  const h = harness((args) => {
    if (args[1] === "query") return [{ id: 1, fields: { "System.ChangedBy": { id: ME } } }];
    if (fail && args.includes("project=Broken")) throw new Error("boom");
    return { value: [] };
  });
  const w = createWatcher(h.deps);
  const targets = [{ org: ORG, project: "Broken" }, { org: ORG, project: PROJECT }];
  await w.cycle(targets, 120_000);
  assert.equal(h.store.has(`watch:wm:${ORG}|Broken`), false);
  assert.equal(h.store.has(`watch:wm:${ORG}|${PROJECT}`), true);
  const before = h.calls.length;
  h.advance(60_000);
  fail = false;
  await w.cycle([targets[0]], 120_000);
  assert.equal(h.calls.length, before, "still backing off");
  h.advance(120_000);
  await w.cycle([targets[0]], 120_000);
  assert.equal(h.store.has(`watch:wm:${ORG}|Broken`), true);
});
