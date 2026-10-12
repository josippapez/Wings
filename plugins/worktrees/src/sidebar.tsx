// The Worktrees sidebar: each open project's git worktrees, what state each is in, and removal after a confirm.
// It runs git from this page (see lib/git.ts) and only offers what git itself would remove without --force.

import { useCallback, useEffect, useRef, useState } from "react";
import { Badge, Button, Card, Checkbox, Empty, Heading, Loading, Row, showSidebar, Spinner, Stack, Text } from "wings-plugin-ui";

import { loadRepos, prune, removeWorktrees, rowsOf, topOf, type Outcome, type Repo } from "./lib/git";
import { age, shortenPath, type Row as WorktreeRow } from "./lib/worktrees";
import "./sidebar.css";

/** What the user is being asked to confirm, inside the page. */
type Confirm = { kind: "remove" | "bulk" | "prune"; root: string; paths: string[] };
type Note = { tone: "success" | "danger"; text: string };

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const name = (path: string) => path.split("/").filter(Boolean).at(-1) ?? path;

function Worktrees() {
  const [repos, setRepos] = useState<Repo[] | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [cwds, setCwds] = useState<string[]>([]);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [deleteBranch, setDeleteBranch] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [notes, setNotes] = useState<Note[]>([]);

  const cwdsRef = useRef<string[]>([]);
  cwdsRef.current = cwds;
  const seq = useRef(0);
  const knownRepos = useRef("");

  const refresh = useCallback(async () => {
    const mine = ++seq.current;
    setLoading(true);
    try {
      const result = await loadRepos(cwdsRef.current);
      if (mine !== seq.current) return;
      setRepos(result.repos);
      setErrors(result.errors);
    } finally {
      if (mine === seq.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    // Only a change in which repos the panes are in reloads; a pane moving inside one repo just re-renders "open in a pane".
    const offPanes = wings.onPanes((list) => {
      const next = [...new Set(list.map((p) => p.cwd).filter(Boolean))].sort();
      if (next.join("\0") === cwdsRef.current.join("\0")) return;
      cwdsRef.current = next;
      setCwds(next);
      void Promise.all(next.map(topOf)).then((tops) => {
        const key = [...new Set(tops)].sort().join("\0");
        if (key === knownRepos.current) return;
        knownRepos.current = key;
        void refresh();
      });
    });
    // Called once with the current state, so it also loads the first time; then each time you come back to Wings.
    const offFocus = wings.onFocus(({ focused }) => {
      if (focused) void refresh();
    });
    return () => {
      offPanes();
      offFocus();
    };
  }, [refresh]);

  const ask = (c: Confirm) => {
    setNotes([]);
    setDeleteBranch(false);
    setConfirm(c);
  };

  const act = async (key: string, work: () => Promise<Note[]>) => {
    setBusy(key);
    setNotes([]);
    try {
      setNotes(await work());
    } catch (e) {
      setNotes([{ tone: "danger", text: (e as Error).message }]);
    } finally {
      setConfirm(null);
      setBusy(null);
      void refresh();
    }
  };

  const summarize = (o: Outcome): Note[] => [
    ...(o.removed.length ? [{ tone: "success" as const, text: `Removed ${plural(o.removed.length, "worktree")}.` }] : []),
    ...o.failed.map((f) => ({ tone: "danger" as const, text: `${shortenPath(f.path)}: ${f.error}` })),
    ...o.branchErrors.map((b) => ({ tone: "danger" as const, text: `Branch ${b.branch} kept: ${b.error}` })),
  ];

  const doConfirm = (repo: Repo) => {
    if (!confirm) return;
    const { kind, paths } = confirm;
    if (kind === "prune") return void act(`prune:${repo.root}`, async () => [{ tone: "success", text: (await prune(repo.root)) || "Nothing to prune." }]);
    void act(`${kind}:${repo.root}`, async () => summarize(await removeWorktrees(repo.root, paths, { deleteBranch, onlyMerged: kind === "bulk" }, () => cwdsRef.current)));
  };

  const openPane = (path: string) =>
    void wings.openPane({ cwd: path }).catch((e: Error) => setNotes([{ tone: "danger", text: e.message }]));

  if (!repos) return <Loading />;
  const removable = repos.reduce((n, r) => n + rowsOf(r, cwds).filter((row) => row.bulk).length, 0);

  return (
    <>
      <Row>
        <Stack gap="none" grow>
          <Text as="div" weight="strong">
            Worktrees
          </Text>
          <Text as="div" tone="muted" truncate>
            {removable ? `${plural(removable, "worktree")} merged and clean` : "Of your open projects"}
          </Text>
        </Stack>
        {loading ? <Spinner label="Refreshing" /> : null}
        <Button variant="outline" size="sm" onClick={() => void refresh()}>
          Refresh
        </Button>
      </Row>
      {notes.map((n, i) => (
        <Text key={i} as="p" tone={n.tone === "danger" ? "danger" : "success"} className="note" role={n.tone === "danger" ? "alert" : "status"}>
          {n.text}
        </Text>
      ))}
      {errors.map((e) => (
        <Text key={e} as="p" tone="warn" className="note">
          {e}
        </Text>
      ))}
      {repos.length === 0 ? <Empty title="No git repo in your panes." description="Open a terminal in a repo and its worktrees show here." /> : null}
      {repos.map((repo) => {
        const rows = rowsOf(repo, cwds);
        const bulk = rows.filter((r) => r.bulk);
        const missing = rows.filter((r) => r.missing && !r.locked);
        const asked = confirm?.root === repo.root ? confirm : null;
        const working = busy?.endsWith(`:${repo.root}`) ?? false;
        return (
          <section key={repo.root} aria-label={name(repo.root)}>
            <Heading level={3}>
              {name(repo.root)}
            </Heading>
            <Text as="div" tone="muted" truncate className="repo-path" title={repo.root}>
              {shortenPath(repo.root)}
              {repo.facts.defaultBranch ? ` · default ${repo.facts.defaultBranch}` : ""}
            </Text>
            {rows.length > 1 && (missing.length > 0 || bulk.length > 0) ? (
              <Row wrap gap="s" className="repo-actions">
                {missing.length > 0 ? (
                  <Button variant="outline" size="xs" disabled={working} onClick={() => ask({ kind: "prune", root: repo.root, paths: missing.map((r) => r.wt.path) })}>
                    {`Prune ${missing.length} missing`}
                  </Button>
                ) : null}
                {bulk.length > 0 ? (
                  <Button variant="outline" size="xs" disabled={working} onClick={() => ask({ kind: "bulk", root: repo.root, paths: bulk.map((r) => r.wt.path) })}>
                    {`Remove ${bulk.length} merged & clean`}
                  </Button>
                ) : null}
              </Row>
            ) : null}
            {asked && asked.kind !== "remove" ? (
              <Card compact className="confirm" role="group" aria-label="Confirm">
                <Text as="div" weight="strong">
                  {asked.kind === "prune" ? `Forget ${plural(asked.paths.length, "worktree record")} whose folder is gone?` : `Remove ${plural(asked.paths.length, "worktree")}?`}
                </Text>
                <ul className="confirm-list">
                  {asked.paths.map((p) => (
                    <li key={p}>
                      <Text mono size="small">
                        {shortenPath(p)}
                      </Text>
                    </li>
                  ))}
                </ul>
                {asked.kind === "bulk" ? <Checkbox checked={deleteBranch} onChange={setDeleteBranch}>Also delete their branches (git branch -d)</Checkbox> : null}
                <Row gap="s">
                  <Button variant="destructive" size="sm" busy={working} onClick={() => doConfirm(repo)}>
                    {asked.kind === "prune" ? "Prune" : "Remove"}
                  </Button>
                  <Button variant="ghost" size="sm" disabled={working} onClick={() => setConfirm(null)}>
                    Cancel
                  </Button>
                </Row>
              </Card>
            ) : null}
            <Stack gap="s" as="ul" className="rows">
              {rows.map((row) => (
                <Item
                  key={row.wt.path}
                  row={row}
                  asked={asked?.kind === "remove" && asked.paths[0] === row.wt.path}
                  working={working}
                  deleteBranch={deleteBranch}
                  setDeleteBranch={setDeleteBranch}
                  onOpen={() => openPane(row.wt.path)}
                  onAsk={() => ask({ kind: "remove", root: repo.root, paths: [row.wt.path] })}
                  onConfirm={() => doConfirm(repo)}
                  onCancel={() => setConfirm(null)}
                />
              ))}
            </Stack>
          </section>
        );
      })}
    </>
  );
}

function Item(p: {
  row: WorktreeRow;
  asked: boolean;
  working: boolean;
  deleteBranch: boolean;
  setDeleteBranch: (v: boolean) => void;
  onOpen: () => void;
  onAsk: () => void;
  onConfirm: () => void;
  onCancel: () => void;
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
      {p.asked ? (
        <Card compact className="confirm" role="group" aria-label="Confirm">
          <Text as="div" weight="strong">
            {`Remove ${shortenPath(wt.path)}?`}
          </Text>
          {wt.branch ? <Checkbox checked={p.deleteBranch} onChange={p.setDeleteBranch}>{`Also delete branch ${wt.branch} (git branch -d)`}</Checkbox> : null}
          <Row gap="s">
            <Button variant="destructive" size="sm" busy={p.working} onClick={p.onConfirm}>
              Remove
            </Button>
            <Button variant="ghost" size="sm" disabled={p.working} onClick={p.onCancel}>
              Cancel
            </Button>
          </Row>
        </Card>
      ) : (
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

showSidebar(<Worktrees />);
