/** The entries of the plugin in `dir`: one for each file at the top of its `src/`. */
export function entries(dir: string): { src: string; out: string }[];

/**
 * Builds the plugin in `dir`: `src/main.ts` to `main.js`, `src/sidebar.tsx` to `sidebar.js`, and so on. With
 * `check`, writes nothing and lists in `stale` the outputs that differ from what's on disk.
 */
export function buildPlugin(
  dir: string,
  options?: { check?: boolean; banner?: string },
): Promise<{ built: { file: string; bytes: number }[]; stale: string[] }>;
