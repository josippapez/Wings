// No Claude session in the panes.
import Sidebar from "../src/pages/sidebar";
import { sessions } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? sessions([]) : undefined);
export default <Sidebar />;
