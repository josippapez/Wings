// The Tasks block with no tasks.
import { BlockView } from "../src/lib/block-view";
import "../src/tasks.css";
import { project } from "./_data";

export const place = "view";
export default <BlockView project={project} tasks={[]} error={null} hideDone={false} onHideDone={() => {}} apply={() => Promise.resolve()} onMove={() => {}} onOpen={() => {}} />;
