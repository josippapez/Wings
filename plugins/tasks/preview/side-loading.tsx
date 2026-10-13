// The Tasks sidebar while loading.
import { SidebarView } from "../src/lib/sidebar-view";
import "../src/tasks.css";
import { project } from "./_data";

export const place = "sidebar";
export default <SidebarView project={project} tasks={null} apply={() => Promise.resolve()} board={() => {}} />;
