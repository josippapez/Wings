// The one source for the colours, radius and font Wings draws with is src/index.css (the app's theme). Plugin pages sit
// in sandboxed iframes that can't read the app's CSS variables, so the values are baked into the stylesheet Wings
// inlines into every page: this copies them into the generated block of packages/plugin-ui/plugin-ui.css as
// --wings-* tokens. wings-plugin-ui keeps that block in the file, so it also works standalone.
//   node scripts/plugin-tokens.mjs           rewrite the generated block
//   node scripts/plugin-tokens.mjs --check   fail if the block differs from what index.css says
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const indexCss = fileURLToPath(new URL("../src/index.css", import.meta.url));
export const pluginUiCss = fileURLToPath(new URL("../../../packages/plugin-ui/plugin-ui.css", import.meta.url));

/** The plugin token and the app theme variable it takes its value from. */
export const MAP = [
  ["--wings-fg", "--foreground"],
  ["--wings-muted", "--muted-foreground"],
  ["--wings-hover", "--hover"],
  ["--wings-pressed", "--pressed"],
  ["--wings-hairline", "--hairline"],
  ["--wings-hairline-strong", "--hairline-strong"],
  ["--wings-ring", "--ring"],
  ["--wings-primary", "--primary"],
  ["--wings-primary-fg", "--primary-foreground"],
  ["--wings-success", "--done"],
  ["--wings-warning", "--working"],
  ["--wings-danger", "--destructive"],
  ["--wings-secondary", "--secondary"],
  ["--wings-border", "--border"],
  ["--wings-control-border", "--input"],
  ["--wings-menu-bg", "--popover"],
  ["--wings-menu-active", "--accent"],
  ["--wings-accent-fg", "--accent-foreground"],
  ["--wings-radius", "--radius"],
  ["--wings-font", "--font-sans"],
];

const START = "/* tokens:start";
const END = "/* tokens:end */";

/** The declarations in the first `selector { ... }` block of `css`, as a Map. */
function declarations(css, selector) {
  const at = css.indexOf(`${selector} {`);
  if (at < 0) throw new Error(`index.css has no ${selector} block`);
  const body = css.slice(css.indexOf("{", at) + 1, css.indexOf("\n}", at));
  return new Map([...body.matchAll(/^\s*(--[\w-]+):\s*(.+?);\s*$/gm)].map((m) => [m[1], m[2]]));
}

/** The generated block for the theme in `source`, the app's index.css. The app is dark, so its .dark values are the theme. */
export function block(source) {
  const theme = new Map([...declarations(source, ":root"), ...declarations(source, ".dark"), ...declarations(source, "@theme inline")]);
  const lines = MAP.map(([token, from]) => {
    const value = theme.get(from);
    if (!value) throw new Error(`index.css has no ${from}, which ${token} is taken from`);
    return `  ${token}: ${value};`;
  });
  return `${START} (generated from apps/desktop/src/index.css by apps/desktop/scripts/plugin-tokens.mjs, do not edit) */\n:root {\n${lines.join("\n")}\n}\n${END}`;
}

/** `css` with its generated block replaced by the one for `source`. */
export function apply(css, source) {
  const from = css.indexOf(START);
  const to = css.indexOf(END);
  if (from < 0 || to < 0) throw new Error("plugin-ui.css has no tokens:start / tokens:end block");
  return css.slice(0, from) + block(source) + css.slice(to + END.length);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const current = readFileSync(pluginUiCss, "utf8");
  const next = apply(current, readFileSync(indexCss, "utf8"));
  if (process.argv.includes("--check")) {
    if (next !== current) {
      console.error("plugin-ui.css tokens are out of date with src/index.css. Run `pnpm tokens` in apps/desktop and commit it.");
      process.exit(1);
    }
  } else if (next !== current) {
    writeFileSync(pluginUiCss, next);
    console.log("plugin-ui.css tokens updated");
  }
}
