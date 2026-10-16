// The Changes sidebar: the changed files as a tree, the files since the branch left main, the commit history, and what goes wrong.
import { changes, files, history } from "./_data";
import { Pressed } from "./_sidebar";

export { default, page } from "../src/pages/sidebar";
export const queries = { changes: () => changes() };

let asked = 0;

export const states = {
  // About forty files in nested folders.
  changes: {},
  // The This branch tab.
  branch: {
    element: <Pressed button="Branch" />,
    queries: {
      changes: ({ mode }: { mode: string }) => (mode === "branch" ? changes({ against: "against main (merge base c3d4e5f)", files: files.slice(0, 12), total: 12, added: 410, removed: 160 }) : changes()),
    },
  },
  // The History tab: merge, branch, tag and remote labels.
  history: { element: <Pressed button="History" />, queries: { ...queries, history: () => history() } },
  // The History tab of a repo with no commits yet.
  "history-empty": { element: <Pressed button="History" />, queries: { ...queries, history: () => history({ rows: [], more: false }) } },
  // A repo with nothing changed.
  empty: { queries: { changes: () => changes({ files: [], total: 0, added: 0, removed: 0 }) } },
  // No git repo in any pane.
  norepo: { queries: { changes: () => ({ repo: null }) } },
  // A git error (an index.lock during a commit) after the list arrived: the list stays, the error is a notice above it.
  error: {
    element: <Pressed button="Refresh" after={300} />,
    queries: {
      changes: () => {
        if (++asked === 1) return changes();
        throw new Error("fatal: Unable to create '/work/acme/web/.git/index.lock': File exists.");
      },
    },
  },
};
