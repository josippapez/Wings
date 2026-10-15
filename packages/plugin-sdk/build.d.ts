/** The entries of the plugin in `dir`: one for each file at the top of its `src/`. */
export function entries(dir: string): { src: string; out: string }[];

/**
 * Builds the plugin in `dir` into its `dist/` folder: `src/main.ts` to `dist/main.js`, `src/sidebar.tsx` to
 * `dist/sidebar.js`, and so on. CSS a page imports becomes `dist/<name>.css`, and a page that mounts with
 * `wings-plugin-ui` or `wings-plugin-sdk/shell` gets a generated `dist/<name>.html` unless the plugin has its
 * own there. A plugin with no `src/main.ts` gets an empty `dist/main.js`. With `check`, writes nothing and lists in
 * `stale` the outputs that differ from what's on disk. With `outDir`, writes there instead of `dist/`.
 */
export function buildPlugin(
  dir: string,
  options?: { check?: boolean; banner?: string; outDir?: string },
): Promise<{ built: { file: string; bytes: number }[]; stale: string[] }>;

/**
 * Builds the plugin in `dir` with the files it ships into `<id>.wings-plugin` in `out` (`dir` by default), the
 * file Wings installs, leaving no built files in `dir`. The package has the same `dist/` layout the manifest
 * points into. Leaves out `src/`, `preview/`, `node_modules`, package and lock files, `tsconfig*.json`,
 * `HANDOVER*`, dotfiles and other `.wings-plugin` files. Resolves its path.
 */
export function pack(dir: string, options?: { out?: string }): Promise<string>;
