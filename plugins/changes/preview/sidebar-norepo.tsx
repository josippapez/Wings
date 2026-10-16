// No git repo in any pane.
import Sidebar from "../src/pages/sidebar";
export const place = "sidebar";
export const queries = { changes: () => ({ repo: null }) };
export default <Sidebar />;
