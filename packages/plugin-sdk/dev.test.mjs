// node --test: dev's rebuilds with a fake watcher and build.
import assert from "node:assert/strict";
import { test } from "node:test";

import { dev } from "./dev.mjs";

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** A dev run with a fake watcher. `builds` counts started builds, and each build waits for `gate` when it is set. */
async function start() {
  const s = { fail: () => false, builds: 0, errors: [], links: 0, gate: null, emit: null };
  const options = {
    watch: (_dir, onChange) => ((s.emit = onChange), { close() {} }),
    build: async () => {
      const n = ++s.builds;
      if (s.gate) await s.gate;
      if (s.fail(n)) throw new Error(`build ${n} failed`);
      return { built: [], stale: [] };
    },
    link: async () => {
      s.links++;
    },
    log: () => {},
    error: (m) => s.errors.push(m),
    debounce: 10,
  };
  s.handle = await dev("/plugin", options);
  s.builds = 0;
  s.links = 0;
  return s;
}

test("a burst of saves makes one build", async () => {
  const s = await start();
  for (const f of ["src/a.ts", "src/b.ts", "src/a.ts"]) s.emit(f);
  await wait(60);
  assert.equal(s.builds, 1);
  assert.equal(s.links, 1);
  s.emit("dist/main.js");
  await wait(40);
  assert.equal(s.builds, 1, "the build's own output isn't a change");
  s.handle.close();
});

test("a save during a build makes one more build", async () => {
  const s = await start();
  let open;
  s.gate = new Promise((r) => (open = r));
  s.emit("src/a.ts");
  await wait(40);
  s.emit("src/b.ts");
  s.emit("src/c.ts");
  await wait(40);
  open();
  await wait(60);
  assert.equal(s.builds, 2);
  s.handle.close();
});

test("a failed build prints its error and keeps watching", async () => {
  const s = await start();
  s.fail = (n) => n === 1;
  s.emit("src/a.ts");
  await wait(40);
  assert.deepEqual(s.errors, ["build 1 failed"]);
  assert.equal(s.links, 0);
  s.emit("src/a.ts");
  await wait(40);
  assert.equal(s.builds, 2);
  assert.equal(s.links, 1);
  s.handle.close();
});
