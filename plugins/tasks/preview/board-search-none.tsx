// A search nothing matches.
import { BoardView } from "../src/ui/board-view";
import "../src/pages/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} layout="board" onLayout={() => {}} byParent={false} folded={[]} onByParent={() => {}} onFold={() => {}} query="zebra" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} />;
