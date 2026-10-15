// A new session: no context yet, no servers, no skills.
import Hud from "../src/pages/sidebar";
import { session, snapshot } from "./_data";

export const place = "sidebar";
export const shared = { hud: snapshot({}, [session({ context: null, servers: [], skills: [] })]) };
export default <Hud />;
