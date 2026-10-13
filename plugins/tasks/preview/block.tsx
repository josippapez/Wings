// The Tasks block with a mix of tasks.
import { BlockView } from "../src/lib/block-view";
import "../src/tasks.css";
import { project, tasks } from "./_data";

void tasks;
export const place = "view";
export default <BlockView project={project} tasks={tasks} error={null} hideDone={false} onHideDone={() => {}} apply={() => Promise.resolve()} onMove={() => {}} onOpen={() => {}} />;
