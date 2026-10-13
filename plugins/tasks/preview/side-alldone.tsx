// The Tasks sidebar when every task is done.
import { SidebarView } from "../src/lib/sidebar-view";
import "../src/tasks.css";
import { project, tasks } from "./_data";

export const place = "sidebar";
export default <SidebarView project={project} tasks={tasks.filter((t) => t.status === "done")} apply={() => Promise.resolve()} board={() => {}} />;
