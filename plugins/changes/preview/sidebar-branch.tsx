// The This branch tab.
import { Sidebar } from "../src/components/Sidebar";
import "../src/sidebar.css";
import { files, state } from "./_data";
export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? state({ mode: "branch", against: "against main (merge base c3d4e5f)", files: files.slice(0, 12), total: 12, added: 410, removed: 160 }) : undefined);
export default <Sidebar />;
