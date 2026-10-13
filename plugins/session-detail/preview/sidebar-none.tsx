// No Claude session in the panes.
import { Sidebar } from "../src/components/Sidebar";
import "../src/sidebar.css";
import { sessions } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? sessions([]) : undefined);
export default <Sidebar />;
