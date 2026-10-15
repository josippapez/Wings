// The Issues tab as a list grouped by status, like Linear: rows with key, type, title, parent and progress.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} layout="list" onLayout={() => {}} byParent folded={[]} onByParent={() => {}} onFold={() => {}} apply={none} send={none} onOpen={() => {}} />;
