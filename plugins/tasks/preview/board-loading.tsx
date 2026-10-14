// The Issues tab while the list loads.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={null} error={null} showCanceled={false} onShowCanceled={() => {}} apply={none} send={none} onOpen={() => {}} />;
