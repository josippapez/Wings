// The Issues tab in rows by parent with the canceled column shown, one row folded, and an error from the last change.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { defaults } from "./_data";

export const fill = true;
export default <BoardView {...defaults} error="A project holds up to 300 issues. Remove one first." showCanceled folded={["WEB-13"]} />;
