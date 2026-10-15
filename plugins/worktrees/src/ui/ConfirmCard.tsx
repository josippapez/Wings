// The card that asks before anything is removed: what git will do, the branches to delete with it, and the two buttons.

import { plural } from "wings-plugin-sdk/format";
import { Button, Card, Checkbox, Row, Text } from "wings-plugin-ui";

import { shortenPath } from "../lib/worktrees";

/** What the user is being asked to confirm: removing one worktree, removing the merged ones, or pruning the missing ones. */
export type Confirm = { kind: "remove" | "bulk" | "prune"; root: string; paths: string[] };

export function ConfirmCard(p: {
  ask: Confirm;
  /** The branch of the one worktree being removed, which can be deleted with it. */
  branch: string | null;
  deleteBranch: boolean;
  onDeleteBranch: (v: boolean) => void;
  working: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { kind, paths } = p.ask;
  const title = kind === "prune" ? `Forget ${plural(paths.length, "worktree record")} whose folder is gone?` : kind === "bulk" ? `Remove ${plural(paths.length, "worktree")}?` : `Remove ${shortenPath(paths[0])}?`;
  const branches = kind === "bulk" ? "Also delete their branches (git branch -d)" : kind === "remove" && p.branch ? `Also delete branch ${p.branch} (git branch -d)` : null;
  return (
    <Card compact className="confirm" role="group" aria-label="Confirm">
      <Text as="div" weight="strong">
        {title}
      </Text>
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
      <Row gap="s">
        <Button variant="destructive" size="sm" busy={p.working} onClick={p.onConfirm}>
          {kind === "prune" ? "Prune" : "Remove"}
        </Button>
        <Button variant="ghost" size="sm" disabled={p.working} onClick={p.onCancel}>
          Cancel
        </Button>
      </Row>
    </Card>
  );
}
