// The Issues block with no issues.
import { BlockView } from "../src/ui/block-view";
import "../src/pages/tasks.css";
import { project } from "./_data";

export const place = "view";
export default <BlockView project={project} issues={[]} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
