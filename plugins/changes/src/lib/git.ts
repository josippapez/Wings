/** Git's empty tree, the base of a repo with no commits yet. */
export const EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
export const COLOR = ["--no-color", "--no-ext-diff", "--no-textconv", "--no-relative"];

/** Repo root by pane cwd; null outside a repo. */
export const roots = new Map<string, Promise<string | null>>();
/** The default branch each repo's "This branch" compares against. */
export const defaults = new Map<string, string | null>();

export async function git(cwd: string, args: string[]) {
  const r = await wings.exec("git", args, { cwd });
  if (r.code !== 0) throw new Error(r.stderr.trim().split("\n")[0] || `git ${args[0]} failed`);
  return r.stdout;
}
export const gitOk = (cwd: string, args: string[]) => wings.exec("git", args, { cwd }).then((r) => (r.code === 0 ? r.stdout.trim() : null));

/** Pathspecs that match these exact paths, not as globs. */
export const literal = (paths: string[]) => paths.map((p) => `:(literal)${p}`);
export const basename = (path: string) => path.split("/").filter(Boolean).at(-1) ?? path;

export async function rootOf(cwd: string): Promise<string | null> {
  if (!cwd) return null;
  if (!roots.has(cwd)) roots.set(cwd, gitOk(cwd, ["rev-parse", "--show-toplevel"]).then((r) => r || null));
  return roots.get(cwd)!;
}

/** `origin/HEAD` as the remote set it, else the usual names. */
export async function defaultBranch(root: string): Promise<string | null> {
  if (defaults.has(root)) return defaults.get(root) ?? null;
  let ref = await gitOk(root, ["rev-parse", "--abbrev-ref", "origin/HEAD"]);
  if (!ref || ref === "origin/HEAD") ref = null;
  for (const name of ["origin/main", "origin/master", "main", "master"]) {
    if (ref) break;
    if (await gitOk(root, ["rev-parse", "--verify", "--quiet", `${name}^{commit}`])) ref = name;
  }
  defaults.set(root, ref);
  return ref;
}
