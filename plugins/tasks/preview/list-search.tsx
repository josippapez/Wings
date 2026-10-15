// The list grouped by status, searched: a word that is only in a description or a log note still finds its issue.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} layout="list" byParent={false} query="rounding totals" />;
