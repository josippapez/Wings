// A repo with nothing changed.
import Sidebar from "../src/pages/sidebar";
import { state } from "./_data";
export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? state({ files: [], total: 0, added: 0, removed: 0 }) : undefined);
export default <Sidebar />;
