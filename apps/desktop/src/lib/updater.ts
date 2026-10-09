import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { toast } from "sonner";

const TOAST_ID = "update";
// One small request to GitHub; a few times a day is plenty for a release feed.
const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;

let installing = false;

async function install(update: Update) {
  installing = true;
  toast.loading(`Installing Wings ${update.version}`, { id: TOAST_ID, duration: Infinity });
  try {
    await update.downloadAndInstall();
    await relaunch();
  } catch (err) {
    installing = false;
    toast.error("Couldn't install the update", { id: TOAST_ID, description: String(err), duration: 8000 });
  }
}

/** `manual` is the menu's Check for Updates: it also says when you're up to date or the check failed. */
export async function checkForUpdate({ manual = false } = {}) {
  if (installing) return;
  if (manual) toast.loading("Checking for updates", { id: TOAST_ID, duration: Infinity });
  try {
    const update = await check();
    if (!update) {
      if (manual) toast.success("Wings is up to date", { id: TOAST_ID, duration: 4000 });
      return;
    }
    toast(`Wings ${update.version} is available`, {
      id: TOAST_ID,
      duration: Infinity,
      description: update.body?.split("\n")[0],
      action: { label: "Install and restart", onClick: () => void install(update) },
    });
  } catch (err) {
    // Offline and rate-limited checks are routine; the next one tries again.
    console.warn("update check failed", err);
    if (manual) toast.error("Couldn't check for updates", { id: TOAST_ID, description: String(err), duration: 8000 });
  }
}

/** Checks for a new release now and every few hours while Wings is open. Returns a stop function. */
export function startUpdateChecks(): () => void {
  if (import.meta.env.DEV) return () => {};
  void checkForUpdate();
  const timer = setInterval(() => void checkForUpdate(), CHECK_EVERY_MS);
  return () => clearInterval(timer);
}
