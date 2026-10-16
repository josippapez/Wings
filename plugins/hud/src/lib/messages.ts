import { channel } from "wings-plugin-sdk/messages";

import type { Snapshot } from "./types";

/** What main serves the sidebar. */
export const ch = channel<{}, {}, { hud: { key: void; value: Snapshot } }>();
