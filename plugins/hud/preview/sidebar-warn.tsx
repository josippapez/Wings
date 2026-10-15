// Limits near and over the warn thresholds, and the context almost full.
import Hud from "../src/pages/sidebar";
import { session, snapshot } from "./_data";

export const place = "sidebar";
export const shared = {
  hud: snapshot({ fiveHour: { percent: 82, resets: "41m", left: "41m", resetsAt: null }, sevenDay: { percent: 97, resets: "1d 3h", left: "1d", resetsAt: null }, memory: { used: 14.8 * 2 ** 30, total: 16 * 2 ** 30, percent: 92 } }, [
    session({ context: { percent: 91, tokens: 182_000, window: 200_000, cache: "Cold" } }),
    session({ id: "s2-bbbb", name: null, project: "api" }),
  ]),
};
export default <Hud />;
