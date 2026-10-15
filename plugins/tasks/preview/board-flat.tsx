// The Issues tab with rows by parent turned off: one board of columns, epics showing how many children are done.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} byParent={false} folded={[]} onByParent={() => {}} onFold={() => {}} apply={none} send={none} onOpen={() => {}} />;
