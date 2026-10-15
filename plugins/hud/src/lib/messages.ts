import { channel } from "wings-plugin-sdk/messages";

import type { Snapshot } from "./types";

/** What main shares with the sidebar. */
export const ch = channel<{}, {}, { hud: Snapshot }>();
