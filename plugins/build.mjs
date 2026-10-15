// Builds every plugin in this folder with wings-plugin-sdk's build: src/main.ts becomes dist/main.js, src/sidebar.tsx
// becomes dist/sidebar.js, and so on. The output is committed, because Wings installs a plugin folder straight from
// the default branch.
//   node build.mjs           build every plugin
//   node build.mjs --check   fail if a committed output differs from what src/ builds
import { readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { buildPlugin } from "wings-plugin-sdk/build";

const root = import.meta.dirname;
const check = process.argv.includes("--check");
const banner = "// Built from src/ by plugins/build.mjs. Edit src/ and run `pnpm build` in plugins/.";

let stale = 0;
for (const d of readdirSync(root, { withFileTypes: true }).filter((d) => d.isDirectory() && d.name !== "node_modules")) {
  const result = await buildPlugin(join(root, d.name), { check, banner });
  for (const { file, bytes } of result.built) console.log(`${relative(root, file)}  ${(bytes / 1024).toFixed(1)} KB`);
  for (const file of result.stale) console.error(`${relative(root, file)} is out of date. Run \`pnpm build\` in plugins/ and commit it.`);
  stale += result.stale.length;
}
if (stale) process.exit(1);
