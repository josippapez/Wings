// The Changes tab with about forty files in nested folders.
import Sidebar from "../src/pages/sidebar";
import { state } from "./_data";
export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? state() : undefined);
export default <Sidebar />;
