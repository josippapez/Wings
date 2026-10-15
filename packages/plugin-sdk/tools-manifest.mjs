// Keeps `contributes.mcpTools` in a plugin's wings-plugin.json equal to the tools in its src/tools.ts, which exports
// `tools`: an object of tool() definitions from wings-plugin-sdk/tools, keyed by tool name. The build runs that file
// in Node, so it and what it imports can't need Wings; a `wings` that does nothing stands in for the real one.
import * as esbuild from "esbuild";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { isDeepStrictEqual } from "node:util";

const MANIFEST = "wings-plugin.json";

/** What a `wings` call does while src/tools.ts loads: nothing, and the result of it does nothing either. */
const stub = new Proxy(() => {}, { get: () => stub, apply: () => stub });

/** The tools of the plugin in `dir`, as `contributes.mcpTools` lists them, or null when it has no src/tools.ts. */
export async function readTools(dir) {
  const entry = join(dir, "src", "tools.ts");
  if (!existsSync(entry)) return null;
  const { outputFiles } = await esbuild.build({ entryPoints: [entry], bundle: true, format: "esm", platform: "node", write: false, logLevel: "warning" });
  const before = globalThis.wings;
  globalThis.wings = stub;
  let loaded;
  try {
    loaded = await import(`data:text/javascript;base64,${Buffer.from(outputFiles[0].text).toString("base64")}`);
  } catch (error) {
    throw new Error(`Couldn't load src/tools.ts to read its tools: ${error.message}. It and what it imports run in Node while the manifest is built, with no page and a wings that does nothing.`);
  } finally {
    globalThis.wings = before;
  }
  if (!loaded.tools || typeof loaded.tools !== "object") throw new Error("src/tools.ts must export `tools`: an object of tool() definitions, keyed by tool name.");
  return Object.entries(loaded.tools).map(([name, { description, input }]) => ({ name, description, inputSchema: input.schema }));
}

/** Where the array that starts at `start` in `text` ends. Only square brackets count, and not the ones in strings. */
function arrayEnd(text, start) {
  let depth = 0;
  let inString = false;
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (inString) {
      if (c === "\\") i++;
      else if (c === '"') inString = false;
    } else if (c === '"') inString = true;
    else if (c === "[") depth++;
    else if (c === "]" && --depth === 0) return i + 1;
  }
}

/**
 * `text`, a wings-plugin.json, with `tools` as its `contributes.mcpTools`. The rest of the file keeps its formatting:
 * only the tools are written over. A manifest that has none yet is written out in full, as 2-space JSON.
 */
function withTools(text, tools) {
  const written = JSON.stringify(tools, null, 2);
  const manifest = JSON.parse(text);
  if (isDeepStrictEqual(manifest.contributes?.mcpTools, JSON.parse(written))) return text;
  const key = /^([ \t]*)"mcpTools"[ \t]*:[ \t]*(?=\[)/m.exec(text);
  if (!key) return `${JSON.stringify({ ...manifest, contributes: { ...manifest.contributes, mcpTools: tools } }, null, 2)}\n`;
  const start = key.index + key[0].length;
  return text.slice(0, start) + written.replaceAll("\n", `\n${key[1]}`) + text.slice(arrayEnd(text, start));
}

/**
 * Writes the tools of src/tools.ts into the manifest. With `check`, writes nothing and lists in `stale` the manifest
 * if it would change. With `outDir`, the manifest is written there, from the one in `dir`. A plugin without a
 * src/tools.ts is left alone.
 */
export async function syncTools(dir, { check = false, outDir = dir } = {}) {
  const built = [];
  const stale = [];
  const tools = await readTools(dir);
  if (!tools) return { built, stale };
  const text = withTools(readFileSync(join(dir, MANIFEST), "utf8"), tools);
  const file = join(outDir, MANIFEST);
  if (existsSync(file) && readFileSync(file, "utf8") === text) return { built, stale };
  if (check) stale.push(file);
  else {
    writeFileSync(file, text);
    built.push({ file, bytes: text.length });
  }
  return { built, stale };
}
