// The Issues tab while the list loads.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={null} error={null} showCanceled={false} onShowCanceled={() => {}} layout="board" onLayout={() => {}} byParent folded={[]} onByParent={() => {}} onFold={() => {}} query="" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} />;
