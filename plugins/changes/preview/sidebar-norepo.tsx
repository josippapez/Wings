// No git repo in any pane.
import { Sidebar } from "../src/components/Sidebar";
import "../src/sidebar.css";
export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? { type: "state", mode: "uncommitted", repo: null } : undefined);
export default <Sidebar />;
