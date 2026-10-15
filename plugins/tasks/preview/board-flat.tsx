// The Issues tab with rows by parent turned off: one board of columns, epics showing how many children are done.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} byParent={false} />;
