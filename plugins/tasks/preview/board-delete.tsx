// The board asking to delete an epic with child issues: delete only it, or it and everything under it.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} deleting="WEB-10" />;
