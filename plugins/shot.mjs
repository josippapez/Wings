// `pnpm shot [filter...] [--width 1000,640] [--height 900] [--out <dir>]`: `wings-plugin shot` over every plugin here.
// The header of packages/plugin-sdk/shot.mjs lists the fixture fields.
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { shot } from "wings-plugin-sdk/shot";

const folders = readdirSync(import.meta.dirname, { withFileTypes: true })
  .filter((d) => d.isDirectory() && d.name !== "node_modules")
  .map((d) => join(import.meta.dirname, d.name));
process.exit(await shot(process.argv.slice(2), folders));
