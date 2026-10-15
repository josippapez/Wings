// One repo on the Worktrees page: its name and path, the buttons that act on several worktrees at once, and a card
// for each worktree. Whatever is being confirmed in the repo shows here too, under those buttons or in its worktree.

import { basename } from "wings-plugin-sdk/format";
import { Button, Heading, Row, Stack, Text } from "wings-plugin-ui";

import type { Repo } from "../lib/git";
import { shortenPath, type Row as WorktreeRow } from "../lib/worktrees";
import { ConfirmCard, type Confirm } from "./ConfirmCard";
import { WorktreeItem } from "./WorktreeItem";

export function RepoSection(p: {
  repo: Repo;
  rows: WorktreeRow[];
  /** What is being confirmed in this repo, if anything. */
  asked: Confirm | null;
  /** The confirmed work is running. */
  working: boolean;
  deleteBranch: boolean;
  onDeleteBranch: (v: boolean) => void;
  onAsk: (c: Confirm) => void;
  onConfirm: () => void;
  onCancel: () => void;
  onOpen: (path: string) => void;
}) {
  const { repo, rows, asked, working } = p;
  const bulk = rows.filter((r) => r.bulk);
  const missing = rows.filter((r) => r.missing && !r.locked);
  const name = basename(repo.root);
  const card = (ask: Confirm, branch: string | null) => (
    <ConfirmCard ask={ask} branch={branch} deleteBranch={p.deleteBranch} onDeleteBranch={p.onDeleteBranch} working={working} onConfirm={p.onConfirm} onCancel={p.onCancel} />
  );
  return (
    <section aria-label={name}>
      <Heading level={3}>{name}</Heading>
      <Text as="div" tone="muted" truncate className="repo-path" title={repo.root}>
        {shortenPath(repo.root)}
        {repo.facts.defaultBranch ? ` · default ${repo.facts.defaultBranch}` : ""}
      </Text>
      {rows.length > 1 && (missing.length > 0 || bulk.length > 0) ? (
        <Row wrap gap="s" className="repo-actions">
          {missing.length > 0 ? (
            <Button variant="outline" size="xs" disabled={working} onClick={() => p.onAsk({ kind: "prune", root: repo.root, paths: missing.map((r) => r.wt.path) })}>
              {`Prune ${missing.length} missing`}
            </Button>
          ) : null}
          {bulk.length > 0 ? (
            <Button variant="outline" size="xs" disabled={working} onClick={() => p.onAsk({ kind: "bulk", root: repo.root, paths: bulk.map((r) => r.wt.path) })}>
              {`Remove ${bulk.length} merged & clean`}
            </Button>
          ) : null}
        </Row>
      ) : null}
      {asked && asked.kind !== "remove" ? card(asked, null) : null}
      <Stack gap="s" as="ul" className="rows">
        {rows.map((row) => (
          <WorktreeItem
            key={row.wt.path}
            row={row}
            working={working}
            confirm={asked?.kind === "remove" && asked.paths[0] === row.wt.path ? card(asked, row.wt.branch) : null}
            onOpen={() => p.onOpen(row.wt.path)}
            onAsk={() => p.onAsk({ kind: "remove", root: repo.root, paths: [row.wt.path] })}
          />
        ))}
      </Stack>
    </section>
  );
}
