// No Claude session in the panes, and no statusline data yet.
import Hud from "../src/pages/sidebar";
import { snapshot } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? snapshot({ fiveHour: null, sevenDay: null, statusline: false }, []) : undefined);
export default <Hud />;
