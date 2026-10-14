// The Issues block with no issues.
import { BlockView } from "../src/lib/block-view";
import "../src/tasks.css";
import { project } from "./_data";

export const place = "view";
export default <BlockView project={project} issues={[]} error={null} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
