// The Issues block with a mix of issues.
import { BlockView } from "../src/ui/block-view";
import "../src/pages/tasks.css";
import { issues, project } from "./_data";

export const place = "view";
export default <BlockView project={project} issues={issues} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
