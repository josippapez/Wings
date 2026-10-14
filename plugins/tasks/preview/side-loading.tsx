// The Issues sidebar while the list loads.
import { SidebarView } from "../src/lib/sidebar-view";
import "../src/tasks.css";
import { project } from "./_data";

export const place = "sidebar";
export default <SidebarView project={project} issues={null} apply={() => Promise.resolve()} openBoard={() => {}} openIssue={() => {}} />;
