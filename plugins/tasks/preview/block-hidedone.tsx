// The Tasks block with done tasks hidden.
import { BlockView } from "../src/lib/block-view";
import "../src/tasks.css";
import { project, tasks } from "./_data";

void tasks;
export const place = "view";
export default <BlockView project={project} tasks={tasks} error={null} hideDone={true} onHideDone={() => {}} apply={() => Promise.resolve()} onMove={() => {}} onOpen={() => {}} />;
