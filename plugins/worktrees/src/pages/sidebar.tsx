// The Worktrees sidebar: the git worktrees of every project and of each repo a pane is in, what state each is in, and
// removal after a confirm. Below them, on request, the repos elsewhere on this Mac that have worktrees (see
// lib/elsewhere.ts). It runs git from this page (see lib/git.ts) and only offers what git itself would remove without --force.

import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage, latestOnly } from "wings-plugin-sdk/async";
import { plural } from "wings-plugin-sdk/format";
import { defaultBranch, onGitChange, repoRoot } from "wings-plugin-sdk/git";
import { Button, Empty, Loading, Notice, Row, Spinner, Stack, Text } from "wings-plugin-ui";

import { elsewhere, elsewhereRepos, findRepoFolders, reposWithWorktrees } from "../lib/elsewhere";
import { loadRepos, prune, removeWorktrees, rowsOf, type Outcome, type Repo } from "../lib/git";
import { shortenPath, type Row as WorktreeRow } from "../lib/worktrees";
import type { Ask } from "../ui/ConfirmCard";
import { Elsewhere, type Scanning } from "../ui/Elsewhere";
import { RepoSection } from "../ui/RepoSection";
import "./sidebar.css";

export const page = { place: "sidebar" };

type Note = { tone: "success" | "danger"; text: string };

/** Starts a refresh. Its `stale()` is true once a later refresh has started, so the earlier one drops what it found. */
const begin = latestOnly();

export default function Worktrees() {
  const [repos, setRepos] = useState<Repo[] | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  // What the last scan found, loaded with git like the repos above: when it finished, and what went wrong loading them.
  const [others, setOthers] = useState<Repo[]>([]);
  const [otherErrors, setOtherErrors] = useState<string[]>([]);
  const [scannedAt, setScannedAt] = useState(0);
  const [scanning, setScanning] = useState<Scanning | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [cwds, setCwds] = useState<string[]>([]);
  const [confirm, setConfirm] = useState<Ask | null>(null);
  const [deleteBranch, setDeleteBranch] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [notes, setNotes] = useState<Note[]>([]);

  const cwdsRef = useRef<string[]>([]);
  cwdsRef.current = cwds;
  // Every project's folder, open in a pane or not. Only pane folders count as "open in a pane".
  const projectsRef = useRef<string[]>([]);
  // null until the first look, so a first look that finds no repo still loads (and shows) the empty state.
  const knownRepos = useRef<string | null>(null);
  const stopScan = useRef<AbortController | null>(null);

  const refresh = useCallback(async () => {
    const stale = begin();
    setLoading(true);
    try {
      const saved = await elsewhere.get();
      // The repos a scan found may be someone else's: only the git commands that run none of a repo's own programs run in them.
      const [result, more] = await Promise.all([loadRepos([...cwdsRef.current, ...projectsRef.current]), loadRepos(saved.folders, { check: false })]);
      if (stale()) return;
      setRepos(result.repos);
      setErrors(result.errors);
      setOthers(more.repos);
      setOtherErrors(more.errors);
      setScannedAt(saved.at);
    } finally {
      if (!stale()) setLoading(false);
    }
  }, []);

  useEffect(() => {
    // Only a change in which repos the projects and panes are in reloads; a pane moving inside one repo just re-renders "open in a pane".
    const look = () =>
      void Promise.all([...cwdsRef.current, ...projectsRef.current].map(repoRoot)).then((tops) => {
        const key = [...new Set(tops)].sort().join("\0");
        if (key === knownRepos.current) return;
        knownRepos.current = key;
        void refresh();
      });
    const offPanes = wings.onPanes((list) => {
      const next = [...new Set(list.map((p) => p.cwd).filter(Boolean))].sort();
      if (next.join("\0") === cwdsRef.current.join("\0")) return;
      cwdsRef.current = next;
      setCwds(next);
      look();
    });
    const offProjects = wings.onProjects((list) => {
      const next = [...new Set(list.map((p) => p.path))].sort();
      if (next.join("\0") === projectsRef.current.join("\0")) return;
      projectsRef.current = next;
      look();
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
      offProjects();
      offFocus();
      offGit();
      stopScan.current?.abort();
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
      const outcome = await removeWorktrees(repo.root, paths, { deleteBranch, onlyMerged: kind === "bulk", check: repo.checked }, () => cwdsRef.current);
      const notes = summarize(outcome);
      // Nothing was removed: the confirm stays so you can read why, and try again or cancel.
      if (!outcome.removed.length && notes.length) throw new Error(notes.map((n) => n.text).join("\n"));
      return notes;
    });
  };

  const openPane = (path: string) =>
    void wings.openPane({ cwd: path }).catch((e) => setNotes([{ tone: "danger", text: errorMessage(e) }]));

  /** Looks for repos on every disk, keeps those with worktrees, and loads them. Stopping it keeps what the last scan found. */
  const scanMac = async () => {
    const stop = new AbortController();
    stopScan.current = stop;
    setScanError(null);
    setScanning({ step: "finding", count: 0 });
    try {
      const folders = await findRepoFolders(stop.signal, (count) => setScanning({ step: "finding", count }));
      setScanning({ step: "checking", count: folders.length });
      await elsewhere.set({ at: Date.now(), folders: await reposWithWorktrees(folders, stop.signal) });
      await refresh();
    } catch (e) {
      if (!stop.signal.aborted) setScanError(errorMessage(e));
    } finally {
      stopScan.current = null;
      setScanning(null);
    }
  };

  if (!repos) return <Loading />;
  const section = (repo: Repo, rows: WorktreeRow[], onOpen?: (path: string) => void) => (
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
      onOpen={onOpen}
    />
  );
  const sections = repos.map((repo) => ({ repo, rows: rowsOf(repo, cwds) }));
  const removable = sections.reduce((n, s) => n + s.rows.filter((row) => row.bulk).length, 0);
  const elsewhereSections = elsewhereRepos(others, repos).map((repo) => ({ repo, rows: rowsOf(repo, cwds) }));

  return (
    <>
      <Row>
        <Stack gap="none" grow>
          <Text as="div" weight="strong">
            Worktrees
          </Text>
          <Text as="div" tone="muted" truncate>
            {removable ? `${plural(removable, "worktree")} merged and clean` : "Of your projects"}
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
      {repos.length === 0 ? <Empty title="None of your projects is a git repo." description="Add a project that is a git repo, or open a terminal in one, and its worktrees show here." /> : null}
      {sections.map(({ repo, rows }) => section(repo, rows, openPane))}
      <Elsewhere at={scannedAt} scanning={scanning} error={scanError} found={elsewhereSections.length} onScan={() => void scanMac()} onStop={() => stopScan.current?.abort()}>
        {otherErrors.map((e) => (
          <Notice key={e} tone="warn" className="note">
            {e}
          </Notice>
        ))}
        {/* Wings opens panes only in your projects, so these have no "Open in new pane". */}
        {elsewhereSections.map(({ repo, rows }) => section(repo, rows))}
      </Elsewhere>
    </>
  );
}
