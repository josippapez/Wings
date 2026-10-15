// The board in rows by parent, searched.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} layout="board" onLayout={() => {}} byParent folded={[]} onByParent={() => {}} onFold={() => {}} query="checkout" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} />;
