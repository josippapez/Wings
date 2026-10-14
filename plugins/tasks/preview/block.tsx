// The Issues block with a mix of issues.
import { BlockView } from "../src/lib/block-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

export const place = "view";
export default <BlockView project={project} issues={issues} error={null} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
