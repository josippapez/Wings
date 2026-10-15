// The Issues tab with a mix of issues, in rows by parent epic or feature.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} byParent folded={[]} onByParent={() => {}} onFold={() => {}} apply={none} send={none} onOpen={() => {}} />;
