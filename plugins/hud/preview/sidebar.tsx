// The HUD sidebar with each kind of session and limits the page has to draw.
import { session, snapshot } from "./_data";

export { default, page } from "../src/pages/sidebar";

export const states = {
  // Limits comfortably under the thresholds, one session with servers and skills.
  ok: { queries: { hud: snapshot() } },
  // Limits near and over the warn thresholds, and the context almost full.
  warn: {
    queries: {
      hud: snapshot({ limits: [{ account: null, fiveHour: { percent: 82, resets: "41m", left: "41m", resetsAt: null }, sevenDay: { percent: 97, resets: "1d 3h", left: "1d", resetsAt: null } }], memory: { used: 14.8 * 2 ** 30, total: 16 * 2 ** 30, percent: 92 } }, [
        session({ context: { percent: 91, tokens: 182_000, window: 200_000, cache: "Cold" } }),
        session({ id: "s2-bbbb", name: null, project: "api" }),
      ]),
    },
  },
  // Two Claude Code accounts, each with limits of its own.
  accounts: {
    queries: {
      hud: snapshot({
        limits: [
          { account: "work", fiveHour: { percent: 88, resets: "1h 12m", left: "1h", resetsAt: null }, sevenDay: { percent: 34, resets: "4d 2h", left: "4d", resetsAt: null } },
          { account: null, fiveHour: { percent: 12, resets: "3h 40m", left: "3h", resetsAt: null }, sevenDay: { percent: 61, resets: "2d 11h", left: "2d", resetsAt: null } },
        ],
      }),
    },
  },
  // The selected pane runs the second session, as the work account: its session shows, and its limits come first.
  selected: {
    queries: {
      hud: snapshot(
        {
          limits: [
            { account: null, fiveHour: { percent: 12, resets: "3h 40m", left: "3h", resetsAt: null }, sevenDay: { percent: 61, resets: "2d 11h", left: "2d", resetsAt: null } },
            { account: "work", fiveHour: { percent: 88, resets: "1h 12m", left: "1h", resetsAt: null }, sevenDay: { percent: 34, resets: "4d 2h", left: "4d", resetsAt: null } },
          ],
        },
        [session({ account: null }), session({ id: "s2-bbbb", name: "invoice export", project: "api", focused: true, account: "work", context: { percent: 23, tokens: 46_000, window: 200_000, cache: "Warm" } })],
      ),
    },
  },
  // The selected session's account hasn't reported limits: the other account's are named, not passed off as its own.
  unreported: {
    queries: {
      hud: snapshot({}, [session({ focused: true, account: "work" })]),
    },
  },
  // A new session: no context yet, no servers, no skills.
  quiet: { queries: { hud: snapshot({}, [session({ context: null, servers: [], skills: [] })]) } },
  // No Claude session in the panes, and no statusline data yet.
  nosession: { queries: { hud: snapshot({ limits: [], statusline: false }, []) } },
};
