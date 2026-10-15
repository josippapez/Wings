// The list by parent, like Linear: each epic over its children, a feature's tasks indented under it, the rest last.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} layout="list" onLayout={() => {}} byParent folded={[]} onByParent={() => {}} onFold={() => {}} query="" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} />;
