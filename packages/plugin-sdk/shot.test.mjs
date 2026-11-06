import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";

import { shot } from "./shot.mjs";

test("fixtures compile WINGS_NATIVE as false and load their browser-only dynamic module", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "wings-shot-test-"));
  const out = join(dir, "shots");
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "package.json"), "{}");
  for (const name of ["preview", "node_modules/wings-plugin-ui", "node_modules/react-dom"]) mkdirSync(join(dir, name), { recursive: true });
  symlinkSync(join(import.meta.dirname, "node_modules", "react"), join(dir, "node_modules", "react"));
  writeFileSync(join(dir, "node_modules", "react-dom", "package.json"), "{}");
  writeFileSync(join(dir, "node_modules", "wings-plugin-ui", "plugin-ui.css"), "");
  writeFileSync(join(dir, "node_modules", "wings-plugin-ui", "index.js"), 'const show = () => document.getElementById("app").append(document.createElement("div"));\nexport const showTab = show;\nexport const showView = show;\nexport const showBlock = show;\nexport const showSidebar = show;\nexport const showPopover = show;\nexport const showSettings = show;');
  writeFileSync(join(dir, "preview", "graph.tsx"), 'void (WINGS_NATIVE ? import("./_graph.native") : import("./_graph.browser"));\nexport const states = { browser: {} };\nexport default function Graph() { return <p>browser graph</p>; }');
  writeFileSync(join(dir, "preview", "_graph.browser.tsx"), 'document.body.dataset.graph = "browser";');
  writeFileSync(join(dir, "preview", "_graph.native.tsx"), 'throw new Error("native graph loaded");');

  assert.equal(await shot(["--width", "20", "--height", "20", "--out", out], [dir]), 0);
  assert.equal(existsSync(join(out, `${basename(dir)}-graph-browser-20.png`)), true);

  const resources = () => readdirSync(tmpdir()).filter((name) => /^wings-shot-(chrome|work)-/.test(name)).sort();
  const before = resources();
  assert.equal(await shot(["--width", "20", "--height", "20", "--out", out, `${basename(dir)}/graph/missing`], [dir]), 2);
  assert.deepEqual(resources(), before);

  writeFileSync(join(dir, "preview", "bad.tsx"), 'import { Missing } from "wings-plugin-ui";\nexport default Missing;');
  assert.equal(await shot(["--width", "20", "--height", "20", "--out", out, `${basename(dir)}/bad`], [dir]), 1);
  assert.deepEqual(resources(), before);
});
