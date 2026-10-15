// The Issues tab with no issues.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={[]} error={null} showCanceled={false} onShowCanceled={() => {}} layout="board" onLayout={() => {}} byParent folded={[]} onByParent={() => {}} onFold={() => {}} query="" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} />;
