// The Issues tab in rows by parent with the canceled column shown, one row folded, and an error from the last change.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error="A project holds up to 300 issues. Remove one first." showCanceled onShowCanceled={() => {}} layout="board" onLayout={() => {}} byParent folded={["WEB-13"]} onByParent={() => {}} onFold={() => {}} apply={none} send={none} onOpen={() => {}} />;
