// The Tasks tab with no tasks.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} tasks={[]} error={null} selected={null} onSelect={() => {}} apply={none} send={none} />;
