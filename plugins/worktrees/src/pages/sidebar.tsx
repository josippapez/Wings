// The Worktrees sidebar: each open project's git worktrees, what state each is in, and removal after a confirm.
// It runs git from this page (see lib/git.ts) and only offers what git itself would remove without --force.

import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage, latestOnly } from "wings-plugin-sdk/async";
import { plural } from "wings-plugin-sdk/format";
import { defaultBranch, onGitChange, repoRoot } from "wings-plugin-sdk/git";
import { Button, Empty, Loading, Notice, Row, Spinner, Stack, Text } from "wings-plugin-ui";

import { loadRepos, prune, removeWorktrees, rowsOf, type Outcome, type Repo } from "../lib/git";
import { shortenPath } from "../lib/worktrees";
import type { Ask } from "../ui/ConfirmCard";
import { RepoSection } from "../ui/RepoSection";
import "./sidebar.css";

export const page = { place: "sidebar" };

type Note = { tone: "success" | "danger"; text: string };

/** Starts a refresh. Its `stale()` is true once a later refresh has started, so the earlier one drops what it found. */
const begin = latestOnly();

export default function Worktrees() {
  const [repos, setRepos] = useState<Repo[] | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [cwds, setCwds] = useState<string[]>([]);
  const [confirm, setConfirm] = useState<Ask | null>(null);
  const [deleteBranch, setDeleteBranch] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [notes, setNotes] = useState<Note[]>([]);

  const cwdsRef = useRef<string[]>([]);
  cwdsRef.current = cwds;
  // null until the first look, so a first look that finds no repo still loads (and shows) the empty state.
  const knownRepos = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    const stale = begin();
    setLoading(true);
    try {
      const result = await loadRepos(cwdsRef.current);
      if (stale()) return;
      setRepos(result.repos);
      setErrors(result.errors);
    } finally {
      if (!stale()) setLoading(false);
    }
  }, []);

  useEffect(() => {
    // Only a change in which repos the panes are in reloads; a pane moving inside one repo just re-renders "open in a pane".
    const offPanes = wings.onPanes((list) => {
      const next = [...new Set(list.map((p) => p.cwd).filter(Boolean))].sort();
      if (next.join("\0") === cwdsRef.current.join("\0")) return;
      cwdsRef.current = next;
      setCwds(next);
      void Promise.all(next.map(repoRoot)).then((tops) => {
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
    // A commit, checkout or pull in any of the repos shown, from anywhere. Dirty state isn't seen: it reloads on focus and after actions.
    const offGit = onGitChange(async (_ids, roots) => {
      const known = knownRepos.current?.split("\0") ?? [];
      if (roots.some((root) => known.includes(root))) await refresh();
    });
    return () => {
      offPanes();
      offFocus();
      offGit();
    };
  }, [refresh]);

  const ask = (c: Ask) => {
    setNotes([]);
    setDeleteBranch(false);
    setConfirm(c);
  };

  /** Runs what the confirm asked for. A throw leaves the confirm open and is shown in it; otherwise it closes and the notes show. */
  const act = async (key: string, work: () => Promise<Note[]>) => {
    setBusy(key);
    setNotes([]);
    try {
      setNotes(await work());
      setConfirm(null);
    } finally {
      setBusy(null);
      void refresh();
    }
  };

  const summarize = (o: Outcome): Note[] => [
    ...(o.removed.length ? [{ tone: "success" as const, text: `Removed ${plural(o.removed.length, "worktree")}.` }] : []),
    ...o.failed.map((f) => ({ tone: "danger" as const, text: `${shortenPath(f.path)}: ${f.error}` })),
    ...o.branchErrors.map((b) => ({ tone: "danger" as const, text: `Branch ${b.branch} kept: ${b.error}` })),
  ];

  const doConfirm = async (repo: Repo) => {
    if (!confirm) return;
    const { kind, paths } = confirm;
    if (kind === "prune") return act(`prune:${repo.root}`, async () => [{ tone: "success", text: (await prune(repo.root)) || "Nothing to prune." }]);
    return act(`${kind}:${repo.root}`, async () => {
      const outcome = await removeWorktrees(repo.root, paths, { deleteBranch, onlyMerged: kind === "bulk" }, () => cwdsRef.current);
      const notes = summarize(outcome);
      // Nothing was removed: the confirm stays so you can read why, and try again or cancel.
      if (!outcome.removed.length && notes.length) throw new Error(notes.map((n) => n.text).join("\n"));
      return notes;
    });
  };

  const openPane = (path: string) =>
    void wings.openPane({ cwd: path }).catch((e) => setNotes([{ tone: "danger", text: errorMessage(e) }]));

  if (!repos) return <Loading />;
  const sections = repos.map((repo) => ({ repo, rows: rowsOf(repo, cwds) }));
  const removable = sections.reduce((n, s) => n + s.rows.filter((row) => row.bulk).length, 0);

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
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            // The button only: focus and pane changes keep what was found.
            repoRoot.forget();
            defaultBranch.forget();
            void refresh();
          }}
        >
          Refresh
        </Button>
      </Row>
      {notes.map((n, i) => (
        <Notice key={i} tone={n.tone} className="note">
          {n.text}
        </Notice>
      ))}
      {errors.map((e) => (
        <Notice key={e} tone="warn" className="note">
          {e}
        </Notice>
      ))}
      {repos.length === 0 ? <Empty title="No git repo in your panes." description="Open a terminal in a repo and its worktrees show here." /> : null}
      {sections.map(({ repo, rows }) => (
        <RepoSection
          key={repo.root}
          repo={repo}
          rows={rows}
          asked={confirm?.root === repo.root ? confirm : null}
          working={busy?.endsWith(`:${repo.root}`) ?? false}
          deleteBranch={deleteBranch}
          onDeleteBranch={setDeleteBranch}
          onAsk={ask}
          onConfirm={() => doConfirm(repo)}
          onCancel={() => setConfirm(null)}
          onOpen={openPane}
        />
      ))}
    </>
  );
}
