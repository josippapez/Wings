// The Issues tab as a list: by parent like Linear, by status, searched, with canceled shown, and asking to delete.
import { BoardView } from "../src/ui/board-view";
import { defaults } from "./_data";

export { page } from "../src/pages/board";

export const states = {
  // Each epic over its children, a feature's tasks indented under it, the rest last.
  parent: { element: <BoardView {...defaults} layout="list" /> },
  // Grouped by status, with Group by parent off.
  status: { element: <BoardView {...defaults} layout="list" byParent={false} /> },
  // Grouped by status, searched: a word that is only in a description or a log note still finds its issue.
  search: { element: <BoardView {...defaults} layout="list" byParent={false} query="rounding totals" /> },
  // With canceled shown, the Done group folded, and an error from the last change.
  canceled: { element: <BoardView {...defaults} error="A project holds up to 300 issues. Remove one first." showCanceled layout="list" folded={["status:done"]} /> },
  // Asking to delete an issue with no children.
  delete: { element: <BoardView {...defaults} layout="list" deleting="WEB-14" /> },
};
