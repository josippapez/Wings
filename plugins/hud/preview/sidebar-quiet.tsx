// A new session: no context yet, no servers, no skills.
import { Hud } from "../src/components/Hud";
import { session, snapshot } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => ((m as { type: string }).type === "hello" ? snapshot({}, [session({ context: null, servers: [], skills: [] })]) : undefined);
export default <Hud />;
