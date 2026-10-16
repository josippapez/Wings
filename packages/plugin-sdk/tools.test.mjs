// node --test: the check a tool's input gets before its handler runs, and the manifest tools the build writes from them.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { registerTools, s, tool } from "./tools.js";
import { readTools, syncTools } from "./tools-manifest.mjs";

let reached = 0;
const handlers = {};
registerTools(
  {
    list: tool({
      description: "Lists things.",
      input: s.object({
        key: s.string({ maxLength: 5 }),
        status: s.enum(["todo", "done"]).optional(),
        limit: s.integer({ minimum: 1, maximum: 200 }).optional(),
        keys: s.array(s.string(), { maxItems: 2 }).optional(),
      }),
      run: (input, context) => {
        reached++;
        return { input, context };
      },
    }),
    ping: tool({ description: "Pings.", run: () => "pong" }),
  },
  { onTool: (name, handler) => (handlers[name] = handler) },
);

test("an input that fits reaches the handler with the pane, and a tool without input takes none", () => {
  assert.deepEqual(handlers.list({ key: "A", status: "done", keys: ["B"] }, { paneId: "p1" }), { input: { key: "A", status: "done", keys: ["B"] }, context: { paneId: "p1" } });
  assert.equal(handlers.ping({}, { paneId: null }), "pong");
  assert.throws(() => handlers.ping({ x: 1 }, { paneId: null }), { message: "Unknown field: x. ping takes no input." });
});

test("an input that doesn't fit throws what to fix, naming the field, and never reaches the handler", () => {
  const before = reached;
  const wrong = [
    [{ key: "A", status: "wip" }, "status must be one of todo, done."],
    [{ key: "toolong" }, "key is limited to 5 characters, this one has 7."],
    [{}, "key is required."],
    [{ key: "A", extra: 1, more: 2 }, "Unknown fields: extra, more. list takes key, status, limit, keys."],
    [{ key: "A", limit: 0 }, "limit must be at least 1, this one is 0."],
    [{ key: "A", limit: 2.5 }, "limit must be a whole number."],
    [{ key: "A", keys: ["B", 1] }, "keys[1] must be text."],
    [{ key: "A", keys: ["B", "C", "D"] }, "keys takes up to 2 items, this one has 3."],
    ["list", "The input must be an object."],
  ];
  for (const [input, message] of wrong) assert.throws(() => handlers.list(input, { paneId: null }), { message });
  assert.equal(reached, before);
  // JSON Schema counts characters, not UTF-16 units: five emoji fit maxLength 5.
  handlers.list({ key: "😀😀😀😀😀" }, { paneId: null });
  assert.equal(reached, before + 1);
});

/** A plugin folder with this package installed, like `npm link wings-plugin-sdk` makes it. */
function plugin(files) {
  const dir = mkdtempSync(join(tmpdir(), "wings-tools-test-"));
  mkdirSync(join(dir, "node_modules"));
  symlinkSync(import.meta.dirname, join(dir, "node_modules", "wings-plugin-sdk"));
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(join(dir, name, ".."), { recursive: true });
    writeFileSync(join(dir, name), text);
  }
  return dir;
}

const TOOLS = `import { s, tool } from "wings-plugin-sdk/tools";
const STATUSES = ["todo", "done"] as const;
// Loading this file runs this line without Wings.
wings.storage.get("seen").then(() => {});
export const tools = {
  list: tool({
    description: "Lists things.",
    input: s.object({ status: s.enum(STATUSES).optional().describe("Only these."), limit: s.integer({ minimum: 1, maximum: 200 }).optional() }),
    run: () => "",
  }),
  ping: tool({ description: "Pings.", run: () => "pong" }),
};
`;
const EXPECTED = [
  { name: "list", description: "Lists things.", inputSchema: { type: "object", properties: { status: { type: "string", enum: ["todo", "done"], description: "Only these." }, limit: { type: "integer", minimum: 1, maximum: 200 } }, additionalProperties: false } },
  { name: "ping", description: "Pings.", inputSchema: { type: "object", properties: {}, additionalProperties: false } },
];

test("the manifest tools are the tools' JSON Schema, in order, and a file that can't be read says why", async (t) => {
  const dir = plugin({ "src/tools.ts": TOOLS });
  const bare = plugin({ "src/main.ts": "" });
  const none = plugin({ "src/tools.ts": "export const other = 1;" });
  const page = plugin({ "src/tools.ts": "window.open();" });
  t.after(() => [dir, bare, none, page].forEach((d) => rmSync(d, { recursive: true })));
  assert.deepEqual(await readTools(dir), EXPECTED);
  assert.equal(globalThis.wings, undefined);
  assert.equal(await readTools(bare), null);
  await assert.rejects(readTools(none), /must export `tools`/);
  await assert.rejects(readTools(page), /Couldn't load src\/tools.ts to read its tools: window is not defined/);
});

test("sync writes the tools into the manifest and nothing else, and check only reports the difference", async (t) => {
  const manifest = (tools) => `{\n  "id": "demo",\n  "contributes": {\n    "mcpTools": ${tools},\n    "views": [\n      { "id": "v", "opens": ["tab", "window"] }\n    ]\n  }\n}\n`;
  const dir = plugin({ "src/tools.ts": TOOLS });
  t.after(() => rmSync(dir, { recursive: true }));
  const file = join(dir, "wings-plugin.json");

  // Tools the manifest already lists are left as they are written, even if the layout isn't the one a build writes.
  const same = manifest(JSON.stringify(EXPECTED));
  writeFileSync(file, same);
  assert.deepEqual(await syncTools(dir, { check: true }), { built: [], stale: [] });
  assert.deepEqual(await syncTools(dir), { built: [], stale: [] });
  assert.equal(readFileSync(file, "utf8"), same);

  const old = manifest('[\n      { "name": "old", "description": "", "inputSchema": { "type": "object" } }\n    ]');
  writeFileSync(file, old);
  assert.deepEqual((await syncTools(dir, { check: true })).stale, [file]);
  assert.equal(readFileSync(file, "utf8"), old);
  assert.equal((await syncTools(dir)).built[0].file, file);
  const written = readFileSync(file, "utf8");
  assert.deepEqual(JSON.parse(written).contributes.mcpTools, EXPECTED);
  assert.match(written, /^\{\n {2}"id": "demo",\n {2}"contributes": \{\n {4}"mcpTools": \[\n {6}\{\n {8}"name": "list",/, "the tools are indented as the key is");
  assert.ok(written.endsWith('\n    ],\n    "views": [\n      { "id": "v", "opens": ["tab", "window"] }\n    ]\n  }\n}\n'), "what follows the tools is as it was");
  assert.deepEqual(await syncTools(dir, { check: true }), { built: [], stale: [] });
});

test("a manifest without tools gets them as 2-space JSON, and outDir takes the manifest to another folder", async (t) => {
  const dir = plugin({ "src/tools.ts": TOOLS, "wings-plugin.json": '{ "id": "demo", "contributes": { "ui": ["badges"] } }' });
  const out = mkdtempSync(join(tmpdir(), "wings-tools-out-"));
  t.after(() => [dir, out].forEach((d) => rmSync(d, { recursive: true })));
  await syncTools(dir, { outDir: out });
  assert.equal(readFileSync(join(dir, "wings-plugin.json"), "utf8"), '{ "id": "demo", "contributes": { "ui": ["badges"] } }');
  assert.equal(readFileSync(join(out, "wings-plugin.json"), "utf8"), `${JSON.stringify({ id: "demo", contributes: { ui: ["badges"], mcpTools: EXPECTED } }, null, 2)}\n`);
});

const FORMATTED = {
  entryId: s.string({ format: "uuid" }),
  day: s.string({ format: "date" }).optional(),
  at: s.string({ format: "date-time" }).optional(),
  hash: s.string({ maxLength: 40, pattern: "^[0-9a-f]{7,40}$" }).optional(),
};

test("format and pattern are written into the manifest schema, and ones that can't be checked fail when the schema is built", async (t) => {
  assert.deepEqual(FORMATTED.hash.schema, { type: "string", maxLength: 40, pattern: "^[0-9a-f]{7,40}$" });
  assert.deepEqual(s.object(FORMATTED).schema.properties.entryId, { type: "string", format: "uuid" });
  assert.throws(() => s.string({ format: "email" }), /format "email" isn't one of uuid, date, date-time/);
  assert.throws(() => s.string({ pattern: "(" }), /Invalid regular expression/);
  const source = `import { s, tool } from "wings-plugin-sdk/tools";\nexport const tools = { get: tool({ description: "Gets.", input: s.object({ id: s.string({ format: "uuid" }).describe("An id."), day: s.string({ format: "date", pattern: "^2026-" }).optional() }), run: () => "" }) };\n`;
  const dir = plugin({ "src/tools.ts": source });
  t.after(() => rmSync(dir, { recursive: true }));
  assert.deepEqual((await readTools(dir))[0].inputSchema.properties, { id: { type: "string", format: "uuid", description: "An id." }, day: { type: "string", format: "date", pattern: "^2026-" } });
});

test("a value that isn't the format or doesn't match the pattern is rejected naming the field, and never reaches the handler", () => {
  const seen = [];
  const registered = {};
  registerTools({ find: tool({ description: "Finds.", input: s.object(FORMATTED), run: (input) => seen.push(input) }) }, { onTool: (name, handler) => (registered[name] = handler) });
  const id = "123E4567-e89b-12d3-a456-426614174000";
  const call = (input) => registered.find(input, { paneId: null });
  call({ entryId: id, day: "2024-02-29", at: "2026-10-08T09:00:00+02:00", hash: "abcdef1" });
  call({ entryId: id, at: "2026-10-08T09:00:00.5Z" });
  assert.equal(seen.length, 2);
  const wrong = [
    [{ entryId: "nope" }, 'entryId must be a uuid, like 123e4567-e89b-12d3-a456-426614174000, this one is "nope".'],
    [{ entryId: id, day: "2026-02-30" }, 'day must be a date as YYYY-MM-DD, this one is "2026-02-30".'],
    [{ entryId: id, day: "26-1-1" }, 'day must be a date as YYYY-MM-DD, this one is "26-1-1".'],
    [{ entryId: id, at: "2026-10-08" }, 'at must be a date and time like 2026-10-08T09:00:00+02:00, this one is "2026-10-08".'],
    [{ entryId: id, hash: "xyz1234" }, 'hash must match the pattern ^[0-9a-f]{7,40}$, this one is "xyz1234".'],
  ];
  for (const [input, message] of wrong) assert.throws(() => call(input), { message });
  assert.equal(seen.length, 2);
});
