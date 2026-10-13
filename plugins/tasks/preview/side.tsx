// The Tasks sidebar with open tasks.
import { SidebarView } from "../src/lib/sidebar-view";
import "../src/tasks.css";
import { project, tasks } from "./_data";

export const place = "sidebar";
export default <SidebarView project={project} tasks={tasks} apply={() => Promise.resolve()} board={() => {}} />;
