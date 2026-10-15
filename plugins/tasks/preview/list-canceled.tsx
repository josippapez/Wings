// The list with canceled shown, the Done group folded, and an error from the last change.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error="A project holds up to 300 issues. Remove one first." showCanceled onShowCanceled={() => {}} layout="list" onLayout={() => {}} byParent folded={["status:done"]} onByParent={() => {}} onFold={() => {}} query="" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} />;
