import { changes, type FileChange } from "./changes";
import { against } from "./diff";
import { basename, gitOk, rootOf } from "./git";
import { compactCommits, historyPage, MAX_LIST_BYTES } from "./history";
import { panes } from "./panes";
import type { FileRow, HistoryMessage, Mode, StateMessage, Tab } from "./types";

export const POLL_MS = 15_000;

/** What the sidebar shows: its tab and mode, the repo it follows, and when it last asked. */
export const sidebar = { tab: "changes" as Tab, mode: "uncommitted" as Mode, root: null as string | null, seen: 0 };
/** The History tab's search and scope, and what its first page last showed, so a poll only sends a change. */
export const historyTab = { query: "", all: false, sig: "" };

/** Sends the History tab a page of commits. A poll sends the first page only when it changed. */
export async function sendHistory(skip: number, { poll = false }: { poll?: boolean } = {}) {
  const root = await sidebarRoot();
  sidebar.root = root;
  const { query, all } = historyTab;
  const base = { type: "history", query, all, skip } as const;
  if (!root) return void (await wings.broadcast({ ...base, repo: null } satisfies HistoryMessage));
  const stale = () => sidebar.root !== root || historyTab.query !== query || historyTab.all !== all;
  try {
    const [branch, head] = await Promise.all([gitOk(root, ["branch", "--show-current"]), gitOk(root, ["rev-parse", "--verify", "--quiet", "HEAD"])]);
    const repo = { root, name: basename(root), branch: branch || (head ? `detached at ${head.slice(0, 7)}` : "no commits") };
    const page = head || all ? await historyPage(root, { query, all, skip }) : { commits: [], more: false };
    if (stale()) return;
    const rows = compactCommits(page.commits);
    if (skip === 0) {
      const sig = JSON.stringify([root, query, all, repo.branch, rows]);
      if (poll && sig === historyTab.sig) return;
      historyTab.sig = sig;
    }
    await wings.broadcast({ ...base, repo, rows, more: page.more || rows.length < page.commits.length } satisfies HistoryMessage);
  } catch (error) {
    if (stale()) return;
    historyTab.sig = "";
    await wings.broadcast({ ...base, repo: { root, name: basename(root) }, error: String((error as Error).message ?? error) } satisfies HistoryMessage);
  }
}

/** The focused pane's repo, else the one the sidebar showed, else the first pane in a repo. */
export async function sidebarRoot() {
  const focused = panes.find((p) => p.focused);
  const root = focused && (await rootOf(focused.cwd).catch(() => null));
  if (root) return root;
  const all = await Promise.all(panes.map((p) => rootOf(p.cwd).catch(() => null)));
  return all.includes(sidebar.root) ? sidebar.root : (all.find(Boolean) ?? null);
}

/** Files as `[status, path, added, removed, oldPath]`, as many as fit in one broadcast. */
function compact(files: FileChange[]) {
  const out: FileRow[] = [];
  let bytes = 0;
  for (const f of files) {
    const row: FileRow = [f.status, f.path, f.added, f.removed, f.oldPath ?? null];
    bytes += JSON.stringify(row).length + 1;
    if (bytes > MAX_LIST_BYTES) break;
    out.push(row);
  }
  return out;
}

/** Sends the sidebar what it shows. */
export async function sendSidebar(options?: { fresh?: boolean }) {
  const root = await sidebarRoot();
  sidebar.root = root;
  const mode = sidebar.mode;
  if (!root) return void (await wings.broadcast({ type: "state", mode, repo: null } satisfies StateMessage));
  void wings.broadcast({ type: "state", mode, loading: true, repo: { root, name: basename(root) } } satisfies StateMessage).catch(() => {});
  try {
    const c = await changes(root, mode, options);
    if (sidebar.root !== root || sidebar.mode !== mode) return;
    const files = compact(c.files);
    await wings.broadcast({
      type: "state",
      mode,
      repo: { root, name: c.name, branch: c.branch },
      against: against(mode, c.baseLabel),
      files,
      total: c.files.length,
      added: c.added,
      removed: c.removed,
      partial: c.partial,
    } satisfies StateMessage);
  } catch (error) {
    if (sidebar.root !== root || sidebar.mode !== mode) return;
    await wings.broadcast({ type: "state", mode, repo: { root, name: basename(root) }, error: String((error as Error).message ?? error) } satisfies StateMessage);
  }
}

export const sidebarOpen = () => Date.now() - sidebar.seen < POLL_MS * 2;
