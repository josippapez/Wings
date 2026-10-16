// The HUD sidebar with each kind of session and limits the page has to draw.
import { session, snapshot } from "./_data";

export { default, page } from "../src/pages/sidebar";

export const states = {
  // Limits comfortably under the thresholds, one session with servers and skills.
  ok: { queries: { hud: snapshot() } },
  // Limits near and over the warn thresholds, and the context almost full.
  warn: {
    queries: {
      hud: snapshot({ fiveHour: { percent: 82, resets: "41m", left: "41m", resetsAt: null }, sevenDay: { percent: 97, resets: "1d 3h", left: "1d", resetsAt: null }, memory: { used: 14.8 * 2 ** 30, total: 16 * 2 ** 30, percent: 92 } }, [
        session({ context: { percent: 91, tokens: 182_000, window: 200_000, cache: "Cold" } }),
        session({ id: "s2-bbbb", name: null, project: "api" }),
      ]),
    },
  },
  // A new session: no context yet, no servers, no skills.
  quiet: { queries: { hud: snapshot({}, [session({ context: null, servers: [], skills: [] })]) } },
  // No Claude session in the panes, and no statusline data yet.
  nosession: { queries: { hud: snapshot({ fiveHour: null, sevenDay: null, statusline: false }, []) } },
};
