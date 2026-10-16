import assert from "node:assert/strict";
import test from "node:test";
import { MAX_FETCH, MAX_NOTES, MAX_SEEN, hitsIn, mentions, notesFor, scan } from "./inbox.ts";

const me = { ids: ["ana"], handles: ["ana"] };
const url = "https://host/team/repo/pull/7";
let n = 0;
const comment = (by, body, createdAt, extra = {}) => ({ id: ++n, replyTo: null, path: null, line: null, side: "additions", author: by, by, body, createdAt, url, ...extra });

test("a handle is a mention only as a whole @name", () => {
  assert.ok(mentions("can you look, @Ana?", ["ana"]));
  assert.ok(mentions("@{abc-123} hi", ["abc-123"]));
  assert.ok(!mentions("@anathema and ana@host.com", ["ana"]));
});

test("a mention on a teammate's PR notifies once, and not again after a restart", async () => {
  const comments = [comment("bob", "@ana can you look at this", "2026-10-16T09:59:00Z")];
  let fetched = 0;
  const run = (inbox, updatedAt) =>
    scan({ inbox, items: [{ url, updatedAt }], mode: "notify", now: "2026-10-16T12:00:00Z", me: async () => me, load: async () => (fetched++, comments) });

  let inbox = { marks: {}, seen: [] };
  // The first list only marks the PR, so a comment from before it was ever seen isn't announced.
  assert.deepEqual(await run(inbox, "2026-10-16T09:00:00Z"), []);
  assert.equal(fetched, 0);
  assert.equal((await run(inbox, "2026-10-16T09:00:00Z")).length, 0, "an unchanged PR is not fetched");
  assert.equal(fetched, 0);

  const hits = await run(inbox, "2026-10-16T10:00:00Z");
  assert.deepEqual(hits.map((h) => [h.kind, h.comment.author]), [["mention", "bob"]]);

  // A restart reads what was stored back. The PR changed again (a label) and the marks were lost: the comment is not announced twice.
  inbox = JSON.parse(JSON.stringify(inbox));
  assert.equal((await run(inbox, "2026-10-16T10:05:00Z")).length, 0);
  inbox.marks[url] = "2026-10-16T09:00:00Z";
  assert.equal((await run(inbox, "2026-10-16T10:06:00Z")).length, 0);
});

test("a new comment in a thread you wrote in is a reply, yours and the provider's are not", () => {
  const root = comment("ana", "why this?", "2026-10-16T08:00:00Z");
  const comments = [
    root,
    comment("bob", "because of X", "2026-10-16T09:00:00Z", { replyTo: root.id }),
    comment("ana", "thanks", "2026-10-16T09:01:00Z", { replyTo: root.id }),
    comment("bob", "Merged main", "2026-10-16T09:02:00Z", { replyTo: root.id, system: true }),
    comment("carl", "unrelated", "2026-10-16T09:03:00Z"),
    comment("bob", "old news", "2026-10-15T09:03:00Z", { replyTo: root.id }),
  ];
  const hits = hitsIn(comments, me, Date.parse("2026-10-16T08:30:00Z"), url);
  assert.deepEqual(hits.map((h) => [h.kind, h.comment.body]), [["reply", "because of X"]]);
});

test("a check fetches only changed PRs, the least recently checked first and at most MAX_FETCH, and tracking fetches none", async () => {
  const items = Array.from({ length: 30 }, (_, i) => ({ url: `u${i}`, updatedAt: "2026-10-16T10:00:00Z" }));
  const inbox = { marks: Object.fromEntries(items.map((i) => [i.url, "2026-10-16T09:00:00Z"])), seen: [] };
  const loaded = [];
  const load = async (u) => (loaded.push(u), []);
  const args = { inbox, items, now: "2026-10-16T12:00:00Z", me: async () => me, load };
  await scan({ ...args, mode: "hold" });
  assert.equal(loaded.length, 0);
  await scan({ ...args, mode: "notify" });
  assert.equal(loaded.length, MAX_FETCH);
  await scan({ ...args, mode: "notify" });
  assert.equal(new Set(loaded).size, 30, "the next check covers the rest");
  inbox.seen = Array.from({ length: MAX_SEEN + 50 }, (_, i) => `k${i}`);
  inbox.marks.u0 = "2026-10-16T09:00:00Z";
  await scan({ ...args, mode: "notify" });
  assert.equal(inbox.seen.length, MAX_SEEN);
});

test("a burst of comments is one notification", () => {
  const note = (i) => ({ hit: { kind: "mention", key: `k${i}`, url, comment: comment("bob", `hi ${i}`, "2026-10-16T09:00:00Z") }, title: `PR ${i}`, args: {} });
  assert.equal(notesFor("web", [note(1), note(2)]).length, 2);
  assert.equal(notesFor("web", Array.from({ length: MAX_NOTES + 1 }, (_, i) => note(i))).length, 1);
});
