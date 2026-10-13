// Limits comfortably under the thresholds, one session with servers and skills.
import { Hud } from "../src/components/Hud";
import { snapshot } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? snapshot() : undefined);
export default <Hud />;
