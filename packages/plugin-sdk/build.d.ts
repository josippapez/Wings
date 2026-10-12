/** The entries of the plugin in `dir`: one for each file at the top of its `src/`. */
export function entries(dir: string): { src: string; out: string }[];

/**
 * Builds the plugin in `dir`: `src/main.ts` to `main.js`, `src/sidebar.tsx` to `sidebar.js`, and so on. CSS a page
 * imports becomes `<name>.css`, and a page that mounts with `wings-plugin-ui` or `wings-plugin-sdk/shell` gets a generated
 * `<name>.html` unless the plugin has its own. With `check`, writes nothing and lists in `stale` the outputs that
 * differ from what's on disk. With `outDir`, writes there instead of `dir`.
 */
export function buildPlugin(
  dir: string,
  options?: { check?: boolean; banner?: string; outDir?: string },
): Promise<{ built: { file: string; bytes: number }[]; stale: string[] }>;

/**
 * Builds the plugin in `dir` with the files it ships into `<id>.wings-plugin` in `out` (`dir` by default), the
 * file Wings installs, leaving no built files in `dir`. Leaves out `src/`, `node_modules`, package and lock
 * files, `tsconfig*.json`, `README*`, `HANDOVER*`, dotfiles and other `.wings-plugin` files. Resolves its path.
 */
export function pack(dir: string, options?: { out?: string }): Promise<string>;
