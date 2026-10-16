#!/usr/bin/env node
// wings-plugin build           builds the plugin in this folder into dist/
// wings-plugin build --check   fails if a file in dist/ differs from what src/ builds
// wings-plugin pack            builds the plugin into <id>.wings-plugin, the file Wings installs, without
//                              writing built files into the folder
// wings-plugin dev            builds on each save and has the running Wings restart the plugin, until you stop it
// Each of them fails with a line for every place wings-plugin.json and the built code disagree.
import { relative } from "node:path";
import { buildPlugin, entries, pack } from "./build.mjs";
import { dev } from "./dev.mjs";

const [command, ...flags] = process.argv.slice(2);
if (command !== "build" && command !== "pack" && command !== "dev") {
  console.error("Usage: wings-plugin build [--check] | wings-plugin pack | wings-plugin dev");
  process.exit(2);
}
const dir = process.cwd();
if (!entries(dir).length) {
  console.error(`No plugin to ${command} here: wings-plugin ${command} runs next to wings-plugin.json, with the code in src/main.ts and src/pages/.`);
  process.exit(1);
}
const fail = (error) => {
  console.error(error.message);
  process.exit(1);
};
if (command === "dev") {
  // The watcher keeps the process alive. Ctrl+C leaves the plugin linked in Wings.
  await dev(dir);
  await new Promise(() => {});
}
if (command === "pack") {
  console.log(relative(dir, await pack(dir).catch(fail)));
  process.exit(0);
}
const check = flags.includes("--check");
const { built, stale } = await buildPlugin(dir, { check }).catch(fail);
for (const { file, bytes } of built) console.log(`${relative(dir, file)}  ${(bytes / 1024).toFixed(1)} KB`);
for (const file of stale) console.error(`${relative(dir, file)} is out of date. Run wings-plugin build and commit it.`);
if (stale.length) process.exit(1);
