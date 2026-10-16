/** Whether an exec entry runs git: `git`, `git branch --show-current`, `git log [--oneline]`. `gitx` and `gh` don't. */
const runsGit = (entry: string) => entry.split("[")[0].trim().split(/\s+/)[0] === "git";

/**
 * The frames that hear that a project's repo changed: the ready ones of plugins that may run git. Their ids tell
 * when you commit or pull, and a plugin that can't read git has no use for them. Main and pages both count.
 */
export function gitListeners<T extends { ready: boolean; plugin: { permissions: { exec: string[] } } }>(frames: Iterable<T>): T[] {
  return [...frames].filter((f) => f.ready && f.plugin.permissions.exec.some(runsGit));
}
