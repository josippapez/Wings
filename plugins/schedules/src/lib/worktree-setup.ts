// A project's worktree setup as the form edits it. The setting lives on the project in Wings, shared with Worktree setup
// in the sidebar and Claude's worktree_setup tool; Schedules keeps no copy, it only reads it and writes the changes back.

/** What `wings.worktreeSetup` returns. */
export interface WorktreeSetup {
  /** The project's own patterns, or null for the default list. */
  include: string[] | null;
  defaults: string[];
  /** The repo has a `.worktreeinclude`, which is used instead of `include`. */
  hasIncludeFile: boolean;
  setup: string | null;
}

/** The fields to change; one left out stays as it is, null resets the list or clears the line. */
export interface WorktreeChange {
  include?: string[] | null;
  setup?: string | null;
}

const lines = (text: string) => text.split("\n").map((l) => l.trim()).filter(Boolean);

/** What the textarea starts with: the project's list, or the defaults when it has none. */
export const includeText = (s: WorktreeSetup) => (s.include ?? s.defaults).join("\n");

/** What saving changes, or undefined when nothing: the default list given back is saved as null, and a field nobody touched is not saved. */
export function worktreeChange(loaded: WorktreeSetup, includeField: string, setupField: string): WorktreeChange | undefined {
  const change: WorktreeChange = {};
  const now = lines(includeField);
  const before = loaded.include ?? loaded.defaults;
  if (now.join("\n") !== before.join("\n")) change.include = now.join("\n") === loaded.defaults.join("\n") ? null : now;
  const setup = setupField.trim();
  if (setup !== (loaded.setup ?? "")) change.setup = setup || null;
  return Object.keys(change).length ? change : undefined;
}
