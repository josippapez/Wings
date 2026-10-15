// The History tab: merge, branch, tag and remote labels.
import Sidebar from "../src/pages/sidebar";
import { history } from "./_data";
export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? history() : undefined);
export default <Sidebar tab="history" />;
