// A working session: running tools, to-dos, edited files, activity.
import Sidebar from "../src/pages/sidebar";
import { idle, sessions, working } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? sessions([working, idle]) : undefined);
export default <Sidebar />;
