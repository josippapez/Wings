// The Tasks tab with a card open for editing, and an error from the last change.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { project, tasks } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} tasks={tasks} error="Tasks are limited to 100 per project." selected="t2" onSelect={() => {}} apply={none} send={none} />;
