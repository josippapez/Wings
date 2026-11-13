// Separate from the browser plugin build. This entry has no manifest, hooks, tools or page mounts.
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(new URL("../../packages/plugin-sdk/package.json", import.meta.url));
const { build } = require("esbuild");
const dir = dirname(fileURLToPath(import.meta.url));
const output = new URL("./dist/native-issues.js", import.meta.url);

export async function buildNative() {
  const result = await build({
    absWorkingDir: dir,
    entryPoints: ["worker/issues.ts"],
    bundle: true,
    format: "iife",
    platform: "neutral",
    target: "es2022",
    write: false,
    banner: { js: "// Built by node plugins/tasks/build-native.mjs. Edit worker/issues.ts, not this file." },
  });
  return result.outputFiles[0].text;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const text = await buildNative();
  if (process.argv.includes("--check")) {
    if (readFileSync(output, "utf8") !== text) throw new Error("plugins/tasks/dist/native-issues.js is out of date.");
  } else {
    writeFileSync(output, text);
  }
}
