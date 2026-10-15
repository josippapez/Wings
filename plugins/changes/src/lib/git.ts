// What this plugin passes to git. Running it is `wings-plugin-sdk/git`.

/** Git's empty tree, the base of a repo with no commits yet. */
export const EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
export const COLOR = ["--no-color", "--no-ext-diff", "--no-textconv", "--no-relative"];

/** Pathspecs that match these exact paths, not as globs. */
export const literal = (paths: string[]) => paths.map((p) => `:(literal)${p}`);
