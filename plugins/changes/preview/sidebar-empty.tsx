// A repo with nothing changed.
import Sidebar from "../src/pages/sidebar";
import { changes } from "./_data";
export const place = "sidebar";
export const answers = { changes: () => changes({ files: [], total: 0, added: 0, removed: 0 }) };
export default <Sidebar />;
