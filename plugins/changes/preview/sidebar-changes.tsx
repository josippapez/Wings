// The Changes tab with about forty files in nested folders.
import Sidebar from "../src/pages/sidebar";
import { changes } from "./_data";
export const place = "sidebar";
export const answers = { changes: () => changes() };
export default <Sidebar />;
