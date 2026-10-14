// The Issues sidebar with open issues.
import { SidebarView } from "../src/lib/sidebar-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

export const place = "sidebar";
export default <SidebarView project={project} issues={issues} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
