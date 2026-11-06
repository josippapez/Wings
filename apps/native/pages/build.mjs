// Builds the native pages into dist/<name>.js: one script each, holding React, the page renderer and the page, for the
// native app's JS engine. Separate from the browser plugin build, like plugins/tasks/build-native.mjs.
//   pages/<name>.tsx            a page, built to dist/<name>.js
//   pages/<plugin>/main.ts      a plugin's main script, built to dist/<plugin>-main.js
//   pages/<plugin>/page.tsx     a plugin's page, built to dist/<plugin>-page.js; so is each `<name>-page.ts[x]`, to
//                               dist/<plugin>-<name>-page.js
//   node apps/native/pages/build.mjs           writes dist/
//   node apps/native/pages/build.mjs --check   fails when dist/ is out of date
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const dir = dirname(fileURLToPath(import.meta.url));
const repo = resolve(dir, "../../..");
const require = createRequire(resolve(repo, "packages/plugin-sdk/package.json"));
const { build } = require("esbuild");

/** The entry files (relative to this folder) and the name each one builds to. */
function entries() {
  const found = readdirSync(dir)
    .filter((file) => file.endsWith(".tsx"))
    .map((file) => ({ file, name: basename(file, ".tsx") }));
  for (const plugin of readdirSync(dir, { withFileTypes: true }).filter((entry) => entry.isDirectory() && entry.name !== "dist")) {
    for (const file of readdirSync(resolve(dir, plugin.name))) {
      if (/^(main|page|.+-page)\.tsx?$/.test(file)) found.push({ file: `${plugin.name}/${file}`, name: `${plugin.name}-${file.replace(/\.tsx?$/, "")}` });
    }
  }
  return found;
}

export async function buildPages() {
  return Promise.all(
    entries().map(async ({ file, name }) => {
      const result = await build({
        absWorkingDir: dir,
        entryPoints: [file],
        bundle: true,
        minify: true,
        format: "iife",
        platform: "neutral",
        target: "es2022",
        jsx: "automatic",
        mainFields: ["module", "main"],
        define: { "process.env.NODE_ENV": '"production"' },
        alias: {
          "wings-plugin-ui/native": resolve(repo, "packages/plugin-ui/native.js"),
          "wings-plugin-sdk/messages": resolve(repo, "packages/plugin-sdk/messages.js"),
          "wings-plugin-sdk/react": resolve(repo, "packages/plugin-sdk/react.js"),
          // Everything that imports `react` must get the copy the renderer uses: two Reacts can't share hooks. The
          // SDK's own `node_modules` has a second copy of the same version.
          react: resolve(repo, "packages/plugin-ui/node_modules/react"),
        },
        nodePaths: [resolve(repo, "packages/plugin-ui/node_modules")],
        write: false,
        banner: { js: `// Built by node apps/native/pages/build.mjs. Edit ${file}, not this file.` },
      });
      return { path: resolve(dir, "dist", `${name}.js`), text: result.outputFiles[0].text };
    }),
  );
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const outputs = await buildPages();
  for (const { path, text } of outputs) {
    if (process.argv.includes("--check")) {
      let current = "";
      try {
        current = readFileSync(path, "utf8");
      } catch {}
      if (current !== text) throw new Error(`${path} is out of date. Run node apps/native/pages/build.mjs.`);
    } else {
      writeFileSync(path, text);
    }
  }
}
