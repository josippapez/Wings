// An idle session with a short history and nothing running.
import Sidebar from "../src/pages/sidebar";
import { idle } from "./_data";

export const place = "sidebar";
export const shared = { sessions: [idle] };
export default <Sidebar />;
