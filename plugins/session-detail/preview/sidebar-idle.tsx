// An idle session with a short history and nothing running.
import { Sidebar } from "../src/components/Sidebar";
import "../src/sidebar.css";
import { idle, sessions } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? sessions([idle]) : undefined);
export default <Sidebar />;
