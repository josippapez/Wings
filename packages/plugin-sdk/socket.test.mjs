// node --test: the socket client against a fake Wings, and its app identifier.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { IDENTIFIER, ask, link, socketPath } from "./socket.mjs";

/** A socket that answers each request line with `reply(request)` after a blank line and an event. */
async function fakeWings(t, reply) {
  const dir = mkdtempSync(join(tmpdir(), "wings-sock-"));
  const path = join(dir, "mcp.sock");
  const seen = [];
  const server = createServer((c) => {
    c.on("data", (d) => {
      const request = JSON.parse(d.toString());
      seen.push(request);
      c.write(`\n${JSON.stringify({ event: "tools_changed" })}\n${JSON.stringify({ id: request.id, ...reply(request) })}\n`);
    });
  });
  await new Promise((r) => server.listen(path, r));
  t.after(() => {
    server.close();
    rmSync(dir, { recursive: true });
  });
  return { path, seen };
}

test("link sends the folder and skips blank lines and events", async (t) => {
  const { path, seen } = await fakeWings(t, () => ({ result: { approved: true } }));
  assert.deepEqual(await link("/p", { path }), { approved: true });
  assert.deepEqual(seen, [{ folder: "/p", op: "link", id: 1 }]);
});

test("errors say what went wrong", async (t) => {
  const old = await fakeWings(t, () => ({ error: "unknown op" }));
  await assert.rejects(ask("link", {}, { path: old.path }), { kind: "unknownOp" });
  const refused = await fakeWings(t, () => ({ error: "bad manifest" }));
  await assert.rejects(ask("link", {}, { path: refused.path }), { kind: "failed", message: "bad manifest" });
  await assert.rejects(ask("link", {}, { path: join(tmpdir(), "no-such-wings.sock") }), { kind: "notRunning" });
});

test("the socket is where Wings puts it, and WINGS_SOCKET overrides", () => {
  assert.equal(socketPath({}, "darwin", "/Users/a"), "/Users/a/Library/Application Support/dev.wings.app/mcp.sock");
  assert.equal(socketPath({ XDG_DATA_HOME: "/x" }, "linux", "/home/a"), "/x/dev.wings.app/mcp.sock");
  assert.equal(socketPath({ WINGS_SOCKET: "/s" }, "darwin", "/Users/a"), "/s");
});

test("the app identifier is the one in tauri.conf.json", () => {
  const config = JSON.parse(readFileSync(join(import.meta.dirname, "../../apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
  assert.equal(IDENTIFIER, config.identifier);
});
