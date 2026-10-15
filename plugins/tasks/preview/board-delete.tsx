// The board asking to delete an epic with child issues: delete only it, or it and everything under it.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} layout="board" onLayout={() => {}} byParent folded={[]} onByParent={() => {}} onFold={() => {}} query="" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} deleting="WEB-10" />;
