// Made-up HUD snapshots.
import type { Overview, SessionView, Snapshot } from "../src/lib/types";

const GB = 2 ** 30;
export const overview = (o: Partial<Overview> = {}): Overview => ({
  fiveHour: { percent: 42, resets: "2h 54m", left: "3h", resetsAt: null },
  sevenDay: { percent: 61, resets: "2d 11h", left: "2d", resetsAt: null },
  statusline: true,
  memory: { used: 9.4 * GB, total: 16 * GB, percent: 59 },
  ...o,
});

export const session = (s: Partial<SessionView> = {}): SessionView => ({
  id: "s1-aaaa",
  project: "web",
  name: "checkout redesign",
  context: { percent: 58, tokens: 116_000, window: 200_000, cache: "Warm" },
  servers: [
    { name: "github", state: "connected", tools: 41, used: true, lastFailed: false },
    { name: "linear", state: "connected", tools: 23, used: false, lastFailed: false },
    { name: "playwright-browser-automation-with-a-long-name", state: "connected", tools: 1, used: true, lastFailed: true },
    { name: "postgres", state: "failed", error: "connection refused: 127.0.0.1:5432", used: false, lastFailed: false },
    { name: "sentry", state: "connecting", used: false, lastFailed: false },
    { name: "notion", state: "sign-in", used: false, lastFailed: false },
    { name: "figma", state: "sign-in", used: false, lastFailed: false },
  ],
  skills: ["wings-page-design", "wings-plugin", "dev-core:opensrc", "frontend-design:frontend-design", "verified:verified-replay-with-a-very-long-skill-name"],
  ...s,
});

export const snapshot = (o: Partial<Overview> = {}, sessions: SessionView[] = [session()]): Snapshot => ({ type: "hud", overview: overview(o), sessions });
