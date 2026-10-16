/** The entries of the plugin in `dir`: `src/main.ts` and one for each file in `src/pages/`. */
export function entries(dir: string): { src: string; out: string }[];

/**
 * Builds the plugin in `dir` into its `dist/` folder: `src/main.ts` to `dist/main.js`, `src/pages/sidebar.tsx` to
 * `dist/sidebar.js`, and so on. A React page (`.tsx`) default-exports its component and exports
 * `page = { place, fill? }`, and the build mounts it with the matching `show*` function of `wings-plugin-ui`.
 * CSS a page imports becomes `dist/<name>.css`, and a page that mounts with `wings-plugin-ui` or
 * `wings-plugin-sdk/shell` gets a generated `dist/<name>.html` unless the plugin has its own there. A plugin with
 * no `src/main.ts` gets an empty `dist/main.js`. With `check`, writes nothing and lists in `stale` the outputs
 * that differ from what's on disk. With `outDir`, writes there instead of `dist/`. A plugin with a `src/tools.ts`
 * gets `contributes.mcpTools` in its manifest written from it, in `manifestDir` (the plugin folder by default).
 *
 * Rejects with a line for each place the manifest and the built code disagree:
 * - a `main` that isn't `dist/main.js`, a page that isn't a file the build writes or the plugin folder has, and an
 *   `icon` that isn't a file in the plugin folder;
 * - a `wings.onTool("name")` and a `contributes.mcpTools` entry that don't match;
 * - a call of `wings.setBadge`, `setPaneStatus`, `openDiff` or `updateDiff` without `badges`, `status` or `diff` in
 *   `contributes.ui`;
 * - an id that no sidebar or view has, in `wings.openView("id")`, `closeView("id")`, `setSidebarLabel("id", ...)` or the
 *   `open: "id"` of a pane status;
 * - an id that no view with `tab` in its `opens` has, in the `open: { view: "id" }` of a project section row or a
 *   notification button.
 *
 * A name the code computes, like `wings.openView(id)`, is skipped, and so is a constant the minifier keeps as a
 * variable, like one used more than once.
 */
export function buildPlugin(
  dir: string,
  options?: { check?: boolean; banner?: string; outDir?: string; manifestDir?: string },
): Promise<{ built: { file: string; bytes: number }[]; stale: string[] }>;

/**
 * Builds the plugin in `dir` with the files it ships into `<id>.wings-plugin` in `out` (`dir` by default), the
 * file Wings installs, leaving no built files in `dir`. The package has the same `dist/` layout the manifest
 * points into. Leaves out `src/`, `preview/`, `node_modules`, package and lock files, `tsconfig*.json`,
 * `HANDOVER*`, dotfiles and other `.wings-plugin` files. Resolves its path, or rejects like `buildPlugin` does.
 */
export function pack(dir: string, options?: { out?: string }): Promise<string>;
