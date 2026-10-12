// Builds a plugin's src/ into the files Wings loads. Every file at the top of src/ is one entry, so src/main.ts
// becomes main.js and src/sidebar.tsx becomes sidebar.js, next to wings-plugin.json. Folders inside src/ hold
// the modules they import. Wings runs plain scripts, so each file is one bundle with its imports, React too.
import * as esbuild from "esbuild";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, parse } from "node:path";

const BANNER = "// Built from src/ by wings-plugin build. Edit src/, not this file.";

/** The entries of the plugin in `dir`: `{ src, out }` for each file at the top of its src/. */
export function entries(dir) {
  const src = join(dir, "src");
  if (!existsSync(join(dir, "wings-plugin.json")) || !existsSync(src)) return [];
  return readdirSync(src, { withFileTypes: true })
    .filter((f) => f.isFile() && /\.(ts|tsx|js|jsx)$/.test(f.name) && !f.name.endsWith(".d.ts"))
    .map((f) => ({ src: join(src, f.name), out: join(dir, `${parse(f.name).name}.js`) }));
}

/**
 * Builds the plugin in `dir`. With `check`, writes nothing and returns the outputs that differ from what's on
 * disk instead, so a release can refuse built files older than their source.
 */
export async function buildPlugin(dir, { check = false, banner = BANNER } = {}) {
  const built = [];
  const stale = [];
  for (const { src, out } of entries(dir)) {
    const result = await esbuild.build({
      entryPoints: [src],
      bundle: true,
      format: "iife",
      // WKWebView on the oldest macOS Wings supports.
      target: "safari16",
      jsx: "automatic",
      define: { "process.env.NODE_ENV": '"production"' },
      banner: { js: banner },
      legalComments: "none",
      minify: true,
      write: false,
      logLevel: "warning",
    });
    const code = result.outputFiles[0].text;
    if (check) {
      if (!existsSync(out) || readFileSync(out, "utf8") !== code) stale.push(out);
    } else {
      writeFileSync(out, code);
      built.push({ file: out, bytes: code.length });
    }
  }
  return { built, stale };
}
