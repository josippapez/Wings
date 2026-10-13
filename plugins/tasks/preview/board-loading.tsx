// The Tasks tab while the list loads.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} tasks={null} error={null} selected={null} onSelect={() => {}} apply={none} send={none} />;
