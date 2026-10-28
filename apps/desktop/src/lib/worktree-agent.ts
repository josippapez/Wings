// New agent in a git worktree. The pieces with no UI or Tauri in them live here so `node --test` can run them.

export type WorktreeAgent = "claude" | "opencode" | "shell";

export const worktreeAgents: { value: WorktreeAgent; label: string }[] = [
  { value: "claude", label: "Claude Code" },
  { value: "opencode", label: "opencode" },
  { value: "shell", label: "Shell" },
];

/** The line typed into the new tab's shell to start the agent; null leaves the plain shell. */
export function agentInput(agent: WorktreeAgent): string | null {
  return agent === "shell" ? null : `${agent}\r`;
}

const ALPHABET = "abcdefghijklmnopqrstuvwxyz0123456789";

/** `wings/` and four random characters: short, and unlikely to clash with a branch that exists. */
export function suggestBranch(random: () => number = Math.random): string {
  return `wings/${Array.from({ length: 4 }, () => ALPHABET[Math.floor(random() * ALPHABET.length)]).join("")}`;
}

/** The branches to start from, the default first. */
export function baseChoices(info: { defaultBranch: string | null; branches: string[] }): string[] {
  return info.defaultBranch ? [info.defaultBranch, ...info.branches.filter((b) => b !== info.defaultBranch)] : info.branches;
}

/** The folder name Wings gives a worktree for `branch`: anything but letters, digits, `.`, `_` and `-` becomes `-`, and none starts or ends with `-` or `.` (same as `slug` in worktree.rs). */
export const worktreeFolder = (branch: string) => branch.replace(/[^A-Za-z0-9._-]/g, "-").replace(/^[-.]+|[-.]+$/g, "");

/** What a worktree row is titled and whether it still needs a branch line: a folder that is just the branch spelled as a path says nothing more, so the branch alone is the title. */
export function worktreeTitle(name: string, branch: string | null | undefined): { title: string; subtitle: string | null } {
  return branch && worktreeFolder(branch) === name ? { title: branch, subtitle: null } : { title: name, subtitle: branch || null };
}

/** Projects in the order shown, each worktree Wings made right after the project it belongs to. A worktree whose project is gone stays in its place as a project. */
export function nestWorktrees<T extends { id: string; parent?: string | null }>(spaces: T[]): { space: T; child: boolean }[] {
  const ids = new Set(spaces.map((s) => s.id));
  const nested = (s: T) => !!s.parent && s.parent !== s.id && ids.has(s.parent);
  return spaces
    .filter((s) => !nested(s))
    .flatMap((s) => [{ space: s, child: false }, ...spaces.filter((c) => nested(c) && c.parent === s.id).map((c) => ({ space: c, child: true }))]);
}

type Listener = () => void;
let requested: string | null = null;
const listeners = new Set<Listener>();

/**
 * Asks to start a new agent in its own worktree of this project: opens the dialog. This is the one function the
 * sidebar menu and the command palette call.
 */
export function requestWorktreeAgent(spaceId: string) {
  requested = spaceId;
  listeners.forEach((l) => l());
}

export function clearWorktreeAgentRequest() {
  requested = null;
  listeners.forEach((l) => l());
}

export const worktreeAgentStore = {
  subscribe: (l: Listener) => {
    listeners.add(l);
    return () => void listeners.delete(l);
  },
  get: () => requested,
};
