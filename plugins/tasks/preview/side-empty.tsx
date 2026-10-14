// The Issues sidebar with no issues.
import { SidebarView } from "../src/lib/sidebar-view";
import "../src/tasks.css";
import { project } from "./_data";

export const place = "sidebar";
export default <SidebarView project={project} issues={[]} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
