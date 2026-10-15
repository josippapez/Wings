// The list by parent, like Linear: each epic over its children, a feature's tasks indented under it, the rest last.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} layout="list" />;
