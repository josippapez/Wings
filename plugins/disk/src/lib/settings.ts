// Settings the Disk usage tab keeps in the plugin's storage. Declared once: two objects for one key don't hear each other.

import { setting } from "wings-plugin-sdk/settings";

/** The folders starred on the overview, as the paths they are scanned with: absolute, or starting with `~`. */
export const starred = setting<string[]>("starred", {
  default: [],
  check: (saved) => (Array.isArray(saved) ? saved.filter((p): p is string => typeof p === "string" && p !== "") : undefined),
});
