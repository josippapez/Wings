// What the sidebar and the graph view get from the main script: the Changes and History tabs, which main serves and keeps
// fresh while they are on screen, the diff viewer and the graph's repo. Main keeps nothing of a page's state but which
// repo it follows.

import { basename } from "wings-plugin-sdk/format";
import { branchLabel, gitOk, repoRoot, watchedProject } from "wings-plugin-sdk/git";

import { changes, fileRow, forgetRepos } from "../lib/changes";
import { openChanges } from "../lib/diff";
import { commitRow, historyPage, openCommit } from "../lib/history";
import { ch, POLL_MS } from "../lib/messages";
import type { HistoryAnswer } from "../lib/types";

/** Answers the pages. `panes` gives the panes Wings last listed. */
export function serveSidebar(panes: () => WingsPane[]) {
  /** The repo the sidebar last showed, which it keeps while the focused pane isn't in one. */
  let shown: string | null = null;
  /** The repo the graph view shows. */
  let graphRoot: string | null = null;
  /** What the panes looked like when `panesChanged` last worked out the sidebar's repo. */
  let seen = "";
  /** The history last loaded is of a repo Wings doesn't watch, so nothing says when it moves. */
  let historyPolled = false;

  /** The focused pane's repo, else the one the sidebar showed while a pane is still in it, else the first pane in a repo. */
  async function sidebarRoot() {
    const list = panes();
    const focused = list.find((p) => p.focused);
    const root = focused && (await repoRoot(focused.cwd));
    if (root) return (shown = root);
    const others = await Promise.all(list.filter((p) => p !== focused).map((p) => repoRoot(p.cwd)));
    return (shown = shown && others.includes(shown) ? shown : (others.find(Boolean) ?? null));
  }

  ch.on("refresh", forgetRepos);

  ch.serve(
    "changes",
    async ({ mode }) => {
      const root = await sidebarRoot();
      if (!root) return { repo: null };
      // A refresh, a refetch and a tab switch all load the same way, so each one reads git again.
      const c = await changes(root, mode, { fresh: true });
      return { repo: { root, name: c.name, branch: c.branch }, against: c.against, total: c.files.length, added: c.added, removed: c.removed, partial: c.partial, files: c.files.map(fileRow) };
    },
    { refetchMs: POLL_MS },
  );

  async function history({ query, all, skip }: { query: string; all: boolean; skip: number }): Promise<HistoryAnswer> {
    const root = await sidebarRoot();
    if (!root) return { repo: null };
    const [branch, head] = await Promise.all([gitOk(root, ["branch", "--show-current"]), gitOk(root, ["rev-parse", "--verify", "--quiet", "HEAD"])]);
    const repo = { root, name: basename(root), branch: branchLabel(branch, head) };
    const page = head || all ? await historyPage(root, { query: query.trim().slice(0, 200), all, skip }) : { commits: [], more: false };
    // A repo no project has isn't watched by Wings, so main's poll reloads its history.
    historyPolled = (await watchedProject(root, await wings.projects())) === null;
    return { repo, rows: page.commits.map(commitRow), more: page.more };
  }
  // No refetch of its own: a git change reloads it, and `poll` a repo Wings doesn't watch.
  ch.serve("history", ({ query, all }) => history({ query, all, skip: 0 }));
  ch.answer("history", history);

  ch.answer("open", ({ root, mode, path }) => openChanges(root, mode, path));

  ch.answer("commit", ({ root, ...commit }) => {
    // It goes to `git show` as an argument, so only a full hash gets there.
    if (!/^[0-9a-f]{40}$/.test(commit.hash)) throw new Error("That isn't a commit hash.");
    return openCommit(root, commit);
  });

  ch.answer("graph", async ({ root }) => {
    graphRoot = root;
    await wings.openView("graph");
    // A graph view that is already open asks again which repo it shows.
    await ch.tell("graph-repo-changed");
  });

  ch.answer("graph-repo", async () => {
    const root = graphRoot ?? (await sidebarRoot());
    return { root, name: root && basename(root) };
  });

  return {
    /** Loads the sidebar's tabs again when the repo it shows moved: a commit, checkout or pull. */
    gitChanged(roots: string[]) {
      if (!shown || !roots.includes(shown)) return;
      ch.refresh("changes");
      ch.refresh("history");
    },
    /** Main's poll: reloads a shown history whose repo Wings doesn't watch. */
    poll() {
      if (historyPolled) ch.refresh("history");
    },
    /** Loads the sidebar's tabs again when the panes changed in a way that moves its repo. */
    async panesChanged() {
      const key = panes()
        .map((p) => `${p.paneId}\0${p.cwd}\0${p.focused}`)
        .join("\n");
      if (key === seen) return;
      seen = key;
      const before = shown;
      if ((await sidebarRoot()) === before) return;
      ch.refresh("changes");
      ch.refresh("history");
    },
  };
}
