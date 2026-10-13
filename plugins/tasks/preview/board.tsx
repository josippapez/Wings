// The Tasks tab with a mix of tasks.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { project, tasks } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} tasks={tasks} error={null} selected={null} onSelect={() => {}} apply={none} send={none} />;
