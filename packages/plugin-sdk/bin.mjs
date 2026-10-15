#!/usr/bin/env node
// wings-plugin build           builds the plugin in this folder into dist/
// wings-plugin build --check   fails if a file in dist/ is older than its source
// wings-plugin pack            builds the plugin into <id>.wings-plugin, the file Wings installs, without
//                              writing built files into the folder
import { relative } from "node:path";
import { buildPlugin, entries, pack } from "./build.mjs";

const [command, ...flags] = process.argv.slice(2);
if (command !== "build" && command !== "pack") {
  console.error("Usage: wings-plugin build [--check] | wings-plugin pack");
  process.exit(2);
}
const dir = process.cwd();
if (!entries(dir).length) {
  console.error(`No plugin to ${command} here: wings-plugin ${command} runs next to wings-plugin.json, with the code in src/.`);
  process.exit(1);
}
if (command === "pack") {
  console.log(relative(dir, await pack(dir)));
  process.exit(0);
}
const check = flags.includes("--check");
const { built, stale } = await buildPlugin(dir, { check });
for (const { file, bytes } of built) console.log(`${relative(dir, file)}  ${(bytes / 1024).toFixed(1)} KB`);
for (const file of stale) console.error(`${relative(dir, file)} is out of date. Run wings-plugin build and commit it.`);
if (stale.length) process.exit(1);
