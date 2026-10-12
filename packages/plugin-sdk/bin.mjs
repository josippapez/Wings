#!/usr/bin/env node
// wings-plugin build           builds the plugin in this folder
// wings-plugin build --check   fails if a built file is older than its source
import { relative } from "node:path";
import { buildPlugin, entries } from "./build.mjs";

const [command, ...flags] = process.argv.slice(2);
if (command !== "build") {
  console.error("Usage: wings-plugin build [--check]");
  process.exit(2);
}
const dir = process.cwd();
if (!entries(dir).length) {
  console.error("No plugin to build here: wings-plugin build runs next to wings-plugin.json, with the code in src/.");
  process.exit(1);
}
const check = flags.includes("--check");
const { built, stale } = await buildPlugin(dir, { check });
for (const { file, bytes } of built) console.log(`${relative(dir, file)}  ${(bytes / 1024).toFixed(1)} KB`);
for (const file of stale) console.error(`${relative(dir, file)} is out of date. Run wings-plugin build and commit it.`);
if (stale.length) process.exit(1);
