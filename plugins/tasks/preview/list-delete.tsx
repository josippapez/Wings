// The list asking to delete an issue with no children.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} layout="list" deleting="WEB-14" />;
