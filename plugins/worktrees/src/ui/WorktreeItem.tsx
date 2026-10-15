// One worktree on the Worktrees page: its branch, where it is, what state it is in, and what can be done with it.

import type { ReactNode } from "react";
import { Badge, Button, Row, Text } from "wings-plugin-ui";

import { age, shortenPath, type Row as WorktreeRow } from "../lib/worktrees";

export function WorktreeItem(p: {
  row: WorktreeRow;
  working: boolean;
  /** The card asking to remove this worktree, which takes the place of its buttons. */
  confirm: ReactNode;
  onOpen: () => void;
  onAsk: () => void;
}) {
  const { row } = p;
  const { wt } = row;
  const flags: { label: string; tone?: "success" | "warn" | "danger" | "info"; title?: string }[] = [];
  if (wt.main) flags.push({ label: "main", tone: "info" });
  if (row.missing) flags.push({ label: "folder missing", tone: "danger", title: wt.prunable ?? undefined });
  if (row.merged) flags.push({ label: "merged", tone: "success" });
  if (row.atDefault) flags.push({ label: "no commits of its own", title: "Its branch is at the default branch's commit" });
  if (row.gone) flags.push({ label: "upstream gone", tone: "warn" });
  if (row.dirty) flags.push({ label: "uncommitted", tone: "warn" });
  if (row.locked) flags.push({ label: "locked", tone: "warn", title: wt.locked || undefined });
  if (row.inPane) flags.push({ label: "open in a pane", tone: "info" });
  const when = age(row.lastCommit, Date.now());
  return (
    <li className="wt" data-removable={row.removable}>
      <Row>
        <Text weight="strong" truncate grow title={wt.branch ?? undefined}>
          {wt.branch ?? (wt.bare ? "(bare)" : wt.head ? `detached ${wt.head.slice(0, 7)}` : "(no branch)")}
        </Text>
        {when ? (
          <Text tone="muted" size="small">
            {when}
          </Text>
        ) : null}
      </Row>
      <Text as="div" tone="muted" size="small" mono truncate title={wt.path}>
        {shortenPath(wt.path)}
      </Text>
      {flags.length ? (
        <Row wrap gap="xs" className="flags">
          {flags.map((f) => (
            <Badge key={f.label} variant={f.tone ? undefined : "outline"} tone={f.tone} title={f.title}>
              {f.label}
            </Badge>
          ))}
        </Row>
      ) : null}
      {p.confirm ?? (
        <Row gap="s" className="item-actions">
          {!row.missing ? (
            <Button variant="ghost" size="xs" onClick={p.onOpen}>
              Open in new pane
            </Button>
          ) : null}
          {row.removable ? (
            <Button variant="outline" size="xs" disabled={p.working} onClick={p.onAsk}>
              Remove
            </Button>
          ) : (
            <Text tone="muted" size="tiny">
              {row.why}
            </Text>
          )}
        </Row>
      )}
    </li>
  );
}
