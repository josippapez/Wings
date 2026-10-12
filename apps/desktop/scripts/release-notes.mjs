// Prints one version's section body from CHANGELOG.md: `node release-notes.mjs 0.1.7`.
// Exits 1 when the version has no section or the section is empty, so a release can't go out without notes.
import { readFileSync } from "node:fs";

const version = process.argv[2]?.replace(/^v/, "");
if (!version) {
  console.error("Usage: release-notes.mjs <version>");
  process.exit(2);
}

const lines = readFileSync(new URL("../../../CHANGELOG.md", import.meta.url), "utf8").split("\n");
const start = lines.findIndex((l) => l.startsWith("## ") && l.slice(3).split(/\s/)[0] === version);
if (start === -1) {
  console.error(`CHANGELOG.md has no section for ${version}`);
  process.exit(1);
}
const rest = lines.slice(start + 1);
const end = rest.findIndex((l) => l.startsWith("## "));
const body = (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
if (!body) {
  console.error(`The ${version} section of CHANGELOG.md is empty`);
  process.exit(1);
}
console.log(body);
