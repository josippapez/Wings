// Limits comfortably under the thresholds, one session with servers and skills.
import Hud from "../src/pages/sidebar";
import { snapshot } from "./_data";

export const place = "sidebar";
export const shared = { hud: snapshot() };
export default <Hud />;
