// The Keep issues local setting: on by default. Saved by the settings page, read by the commit hook.

import { setting } from "wings-plugin-sdk/settings";

export const keepLocal = setting<boolean>("keepLocal", { default: true, check: (saved) => (typeof saved === "boolean" ? saved : undefined) });
