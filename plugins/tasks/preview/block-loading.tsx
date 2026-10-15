// The Issues block while the list loads.
import { BlockView } from "../src/ui/block-view";
import "../src/pages/tasks.css";
import { project } from "./_data";

export const place = "view";
export default <BlockView project={project} issues={null} error={null} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
