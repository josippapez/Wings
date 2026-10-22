// The card that asks before anything is removed: what git will do, the branches to delete with it, and the two buttons.

import { plural } from "wings-plugin-sdk/format";
import { Checkbox, Confirm, Notice, Text } from "wings-plugin-ui";

import { shortenPath } from "../lib/worktrees";

/** What the user is being asked to confirm: removing one worktree, removing the merged ones, or pruning the missing ones. */
export type Ask = { kind: "remove" | "bulk" | "prune"; root: string; paths: string[] };

/**
 * A repo's own settings and hooks can make git run programs, and `git worktree remove` (which looks for changes first)
 * and `git branch -d` do. That is why removing is a warning in a repo that isn't one of your projects.
 */
const OUTSIDE_WARNING = "This repo is outside your projects. Removing a worktree, and its branch if you tick that, runs git in it, and a repo's own settings and hooks can make git run programs. Only remove worktrees of repos you trust.";

export function ConfirmCard(p: {
  ask: Ask;
  /** The branch of the one worktree being removed, which can be deleted with it. */
  branch: string | null;
  /** The repo is outside your projects, so it may not be yours: removing a worktree in it warns first. */
  outside?: boolean;
  deleteBranch: boolean;
  onDeleteBranch: (v: boolean) => void;
  /** Removes or prunes. It rejects with the reason when nothing could be done: the card then stays open and shows it. */
  onConfirm: () => Promise<unknown>;
  onCancel: () => void;
}) {
  const { kind, paths } = p.ask;
  const title = kind === "prune" ? `Forget ${plural(paths.length, "worktree record")} whose folder is gone?` : kind === "bulk" ? `Remove ${plural(paths.length, "worktree")}?` : `Remove ${shortenPath(paths[0])}?`;
  const branches = kind === "bulk" ? "Also delete their branches (git branch -d)" : kind === "remove" && p.branch ? `Also delete branch ${p.branch} (git branch -d)` : null;
  return (
    <Confirm title={title} cancelLabel="Cancel" onCancel={p.onCancel} actions={[{ id: kind, label: kind === "prune" ? "Prune" : "Remove", onConfirm: p.onConfirm }]}>
      {kind === "remove" && p.outside ? <Notice tone="warn">{OUTSIDE_WARNING}</Notice> : null}
      {kind !== "remove" ? (
        <ul className="confirm-list">
          {paths.map((path) => (
            <li key={path}>
              <Text mono size="small">
                {shortenPath(path)}
              </Text>
            </li>
          ))}
        </ul>
      ) : null}
      {branches ? (
        <Checkbox checked={p.deleteBranch} onChange={p.onDeleteBranch}>
          {branches}
        </Checkbox>
      ) : null}
    </Confirm>
  );
}
