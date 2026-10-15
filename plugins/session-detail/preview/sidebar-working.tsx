// A working session: running tools, to-dos, edited files, activity.
import Sidebar from "../src/pages/sidebar";
import { detail, idle, working } from "./_data";

export const place = "sidebar";
export const shared = { sessions: [working, idle] };
export const answers = { detail };
export default <Sidebar />;
