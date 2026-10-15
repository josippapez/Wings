// The list grouped by status, with Group by parent off.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} layout="list" byParent={false} />;
