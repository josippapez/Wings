// The Issues tab with no issues.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} issues={[]} />;
