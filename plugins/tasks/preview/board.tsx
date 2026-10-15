// The Issues tab as a board: in rows by parent epic or feature, in one board of columns, searched, empty, loading, and as the page runs it.
import BoardPage, { page } from "../src/pages/board";
import { BoardView } from "../src/ui/board-view";
import { defaults, project, storage } from "./_data";

export { page };

export const states = {
  // A mix of issues, in rows by parent epic or feature.
  parent: { element: <BoardView {...defaults} /> },
  // Rows by parent turned off: one board of columns, epics showing how many children are done.
  flat: { element: <BoardView {...defaults} byParent={false} /> },
  // In rows by parent, searched.
  search: { element: <BoardView {...defaults} query="checkout" /> },
  // A search nothing matches.
  "search-none": { element: <BoardView {...defaults} byParent={false} query="zebra" /> },
  // In rows by parent with the canceled column shown, one row folded, and an error from the last change.
  canceled: { element: <BoardView {...defaults} error="A project holds up to 300 issues. Remove one first." showCanceled folded={["WEB-13"]} /> },
  // Asking to delete an epic with child issues: delete only it, or it and everything under it.
  delete: { element: <BoardView {...defaults} deleting="WEB-10" /> },
  // No issues.
  empty: { element: <BoardView {...defaults} issues={[]} /> },
  // While the list loads.
  loading: { element: <BoardView {...defaults} issues={null} /> },
  // As the page runs it: its issues read from storage, its layout from the Default view setting (the board).
  page: { element: <BoardPage />, project, storage },
};
