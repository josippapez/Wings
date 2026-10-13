import test from "node:test";
import assert from "node:assert/strict";
import { applyOp, baseOf, emptyInfo, fromLegacy, LIMITS, parseInfo, renderInfo, sizeOf } from "./model.ts";

let n = 0;
const id = () => `id${++n}`;
const run = (info, op, by = "agent", now = 1) => applyOp(info, op, by, now, id);
const section = (info, title) => info.sections.find((s) => s.title === title);

test("migration turns the old notes into a Notes section by you", () => {
  const info = fromLegacy("  old text\n", 5);
  assert.equal(info.sections.length, 1);
  assert.deepEqual([info.sections[0].title, info.sections[0].body, info.sections[0].by], ["Notes", "old text", "you"]);
  assert.equal(fromLegacy("   ", 5).sections.length, 0);
});

test("migrating text longer than a section splits it, losing nothing", () => {
  const text = Array.from({ length: 40 }, (_, i) => `para ${i} ${"x".repeat(500)}`).join("\n\n");
  const info = fromLegacy(text, 1);
  assert.ok(info.sections.length >= 2);
  assert.ok(info.sections.every((s) => s.body.length <= LIMITS.body));
  assert.equal(info.sections.map((s) => s.body).join("\n\n"), text);
  assert.equal(info.sections[1].title, "Notes (2)");
});

test("parseInfo accepts the stored shape and rejects others", () => {
  const info = run(emptyInfo(), { kind: "setFact", key: "Port", value: "5173" });
  assert.deepEqual(parseInfo(JSON.parse(JSON.stringify(info))), info);
  assert.equal(parseInfo({ text: "old", updatedAt: 1 }), null);
  assert.equal(parseInfo(null), null);
});

test("facts: set, replace by key ignoring case, rename, remove, one line", () => {
  let info = run(emptyInfo(), { kind: "setFact", key: "Dev server", value: "pnpm dev\n port 5173" });
  assert.equal(info.facts[0].value, "pnpm dev port 5173");
  info = run(info, { kind: "setFact", key: "dev SERVER", value: "pnpm start" });
  assert.equal(info.facts.length, 1);
  assert.equal(info.facts[0].key, "dev SERVER");
  info = run(info, { kind: "setFact", key: "Port", value: "1" });
  info = run(info, { kind: "setFact", key: "Server", value: "pnpm start", oldKey: "dev SERVER" }, "you");
  assert.deepEqual(info.facts.map((f) => f.key), ["Server", "Port"]);
  assert.throws(() => run(info, { kind: "setFact", key: "port", value: "x", oldKey: "Server" }), /already a fact/);
  assert.throws(() => run(info, { kind: "removeFact", key: "nope" }), /No fact/);
  assert.equal(run(info, { kind: "removeFact", key: "PORT" }).facts.length, 1);
  assert.throws(() => run(info, { kind: "setFact", key: "a", value: "x".repeat(LIMITS.value + 1) }), /limited/);
});

test("fact count is capped", () => {
  let info = emptyInfo();
  for (let i = 0; i < LIMITS.facts; i++) info = run(info, { kind: "setFact", key: `k${i}`, value: "v" });
  assert.throws(() => run(info, { kind: "setFact", key: "one more", value: "v" }), /limited to 40 facts/);
  assert.equal(run(info, { kind: "setFact", key: "k0", value: "changed" }).facts.length, LIMITS.facts);
});

test("write_section creates, replaces its own, and refuses a section you last wrote", () => {
  let info = run(emptyInfo(), { kind: "writeSection", title: "Architecture", body: "v1" });
  assert.equal(section(info, "Architecture").by, "agent");
  info = run(info, { kind: "writeSection", title: "architecture", body: "v2" });
  assert.equal(info.sections.length, 1);
  assert.equal(section(info, "Architecture").body, "v2");
  const mine = run(info, { kind: "editSection", id: info.sections[0].id, title: "Architecture", body: "mine", base: baseOf("v2") }, "you");
  assert.equal(section(mine, "Architecture").by, "you");
  assert.throws(() => run(mine, { kind: "writeSection", title: "Architecture", body: "clobber" }), /use append/i);
  assert.equal(section(mine, "Architecture").body, "mine");
});

test("append creates a section, adds to one, and keeps your authorship", () => {
  let info = run(emptyInfo(), { kind: "appendSection", title: "Gotchas", body: "one" });
  assert.equal(section(info, "Gotchas").by, "agent");
  info = run(info, { kind: "appendSection", title: "Gotchas", body: "two" });
  assert.equal(section(info, "Gotchas").body, "one\n\ntwo");
  const mine = run(info, { kind: "editSection", id: info.sections[0].id, title: "Gotchas", body: "mine", base: baseOf(info.sections[0].body) }, "you");
  const after = run(mine, { kind: "appendSection", title: "Gotchas", body: "more" });
  assert.equal(section(after, "Gotchas").body, "mine\n\nmore");
  assert.equal(section(after, "Gotchas").by, "you");
});

test("a page's edit keeps what an agent appended since the page read the section", () => {
  let info = run(emptyInfo(), { kind: "writeSection", title: "A", body: "base" });
  const base = info.sections[0].body;
  info = run(info, { kind: "appendSection", title: "A", body: "agent adds" });
  const edited = run(info, { kind: "editSection", id: info.sections[0].id, title: "A", body: "base edited", base: baseOf(base) }, "you");
  assert.equal(edited.sections[0].body, "base edited\n\nagent adds");
  // Without an append in between, the edit is the text.
  const plain = run(edited, { kind: "editSection", id: edited.sections[0].id, title: "A", body: "new", base: baseOf(edited.sections[0].body) }, "you");
  assert.equal(plain.sections[0].body, "new");
});

test("an edit that changes nothing leaves the section's author alone", () => {
  const info = run(emptyInfo(), { kind: "writeSection", title: "A", body: "x" });
  const same = run(info, { kind: "editSection", id: info.sections[0].id, title: "A", body: "x", base: baseOf("x") }, "you");
  assert.equal(same, info);
});

test("sections: add, duplicate title, move, delete, caps", () => {
  let info = run(emptyInfo(), { kind: "addSection", title: "One" }, "you");
  info = run(info, { kind: "addSection", title: "Two" }, "you");
  assert.throws(() => run(info, { kind: "addSection", title: "one" }, "you"), /already a section/);
  info = run(info, { kind: "moveSection", id: info.sections[1].id, dir: -1 }, "you");
  assert.deepEqual(info.sections.map((s) => s.title), ["Two", "One"]);
  assert.equal(run(info, { kind: "moveSection", id: info.sections[0].id, dir: -1 }, "you"), info);
  assert.equal(run(info, { kind: "deleteSection", id: info.sections[0].id }, "you").sections.length, 1);
  assert.throws(() => run(info, { kind: "writeSection", title: "Big", body: "x".repeat(LIMITS.body + 1) }), /Split it/);
  let many = emptyInfo();
  for (let i = 0; i < LIMITS.sections; i++) many = run(many, { kind: "addSection", title: `s${i}` }, "you");
  assert.throws(() => run(many, { kind: "appendSection", title: "extra", body: "x" }), /limited to 20 sections/);
});

test("the total per project is capped, and deleting still works", () => {
  let info = emptyInfo();
  for (let i = 0; i < 4; i++) info = run(info, { kind: "writeSection", title: `S${i}`, body: "x".repeat(LIMITS.body) });
  assert.ok(sizeOf(info) <= LIMITS.total);
  assert.throws(() => run(info, { kind: "writeSection", title: "S4", body: "x".repeat(LIMITS.body) }), /in all/);
  assert.equal(run(info, { kind: "deleteSection", id: info.sections[0].id }).sections.length, 3);
});

test("read lists facts and sections as Markdown, saying who wrote them", () => {
  let info = run(emptyInfo(), { kind: "setFact", key: "Port", value: "5173" });
  info = run(info, { kind: "writeSection", title: "Release", body: "tag it" });
  const out = renderInfo(info, "Wings");
  assert.match(out, /^# Project info: Wings/);
  assert.match(out, /- Port: 5173/);
  assert.match(out, /## Release\n_Last written by an agent._\n\ntag it/);
  assert.equal(renderInfo(emptyInfo(), "Wings"), "No project info yet for Wings.");
});

test("append can't grow a section past the section cap, so the user can still edit it", () => {
  let info = run(emptyInfo(), { kind: "appendSection", title: "Log", body: "a".repeat(LIMITS.body - 1000) });
  assert.throws(() => run(info, { kind: "appendSection", title: "Log", body: "b".repeat(2000) }), /Start a new section/);
  assert.equal(info.sections[0].body.length, LIMITS.body - 1000);
  info = run(info, { kind: "appendSection", title: "Log", body: "b".repeat(500) });
  assert.ok(info.sections[0].body.length <= LIMITS.body);
});

test("an edit plus what an agent appended meanwhile can't pass the section cap", () => {
  let info = run(emptyInfo(), { kind: "writeSection", title: "A", body: "x".repeat(LIMITS.body - 100) });
  const { id } = info.sections[0];
  const base = baseOf(info.sections[0].body);
  info = run(info, { kind: "appendSection", title: "A", body: "y".repeat(50) });
  assert.throws(() => run(info, { kind: "editSection", id, title: "A", body: "x".repeat(LIMITS.body - 10), base }, "you"), /would pass/);
});

test("the largest page edit fits in a broadcast even when JSON escapes every character", () => {
  const message = { type: "op", projectId: "p".repeat(64), rid: "r".repeat(36), op: { kind: "editSection", id: "abcdefgh", title: "\u0001".repeat(LIMITS.title), body: "\u0001".repeat(LIMITS.body), base: baseOf("x".repeat(LIMITS.body)) } };
  assert.ok(JSON.stringify(message).length < 64_000);
});

test("a fingerprint tells an unchanged start from a changed one", () => {
  let info = run(emptyInfo(), { kind: "writeSection", title: "A", body: "hello" });
  const { id } = info.sections[0];
  info = run(info, { kind: "appendSection", title: "A", body: "more" });
  // The page read "HELLO" (not what is stored): its edit replaces, nothing is added after it.
  const out = run(info, { kind: "editSection", id, title: "A", body: "mine", base: baseOf("HELLO") }, "you");
  assert.equal(out.sections[0].body, "mine");
});

test("moveSection takes only 1 or -1", () => {
  const info = run(emptyInfo(), { kind: "addSection", title: "A" }, "you");
  for (const dir of [0, 2, -3, "1", null]) assert.throws(() => run(info, { kind: "moveSection", id: info.sections[0].id, dir }, "you"), /dir is 1 or -1/);
});
