import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { api, type AvailablePlugin, type PluginView } from "@/lib/api";
import { notificationSound } from "@/lib/sound";

const TOAST_ID = "plugin-updates";
// Same cadence as the app's own update check; each plugin is one small request to GitHub.
const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;
const FIRST_CHECK_MS = 20_000;
const NEW_TOAST_ID = "plugins-available";
/** Ids of the repo plugins already announced, so each new one is toasted once, ever. */
const SEEN_KEY = "wings.plugins.seen";

/** The repo plugins that aren't installed and weren't announced before. */
const unseenPlugins = (available: AvailablePlugin[], installed: PluginView[], seen: string[]) =>
  available.filter((a) => !seen.includes(a.id) && !installed.some((p) => p.id === a.id));

/** Toasts the repo plugins that are new since the last check, once each. */
function announceNew(list: AvailablePlugin[], installed: PluginView[], openPlugins: () => void) {
  const seen: string[] = JSON.parse(localStorage.getItem(SEEN_KEY) ?? "[]");
  const unseen = unseenPlugins(list, installed, seen);
  localStorage.setItem(SEEN_KEY, JSON.stringify([...new Set([...seen, ...list.map((a) => a.id)])]));
  if (unseen.length === 0) return;
  toast(unseen.length === 1 ? `New plugin: ${unseen[0].name}` : `${unseen.length} new plugins`, {
    id: NEW_TOAST_ID,
    duration: Infinity,
    description: unseen.length === 1 ? unseen[0].description : unseen.map((a) => a.name).join(", "),
    action: { label: "Review", onClick: openPlugins },
  });
}

/** The newer version of a plugin, if `latest` is one. */
export const newerVersion = (plugin: PluginView, latest: string | undefined) => (latest && latest !== plugin.version.replace(/^v/, "") ? latest : null);

/**
 * Looks for newer releases of the GitHub plugins on start and every few hours, and on `check(true)`.
 * `updates` maps a plugin id to the version `api.pluginLatestVersion` reported. A new version is toasted once per session.
 * `available` lists the Wings repo's plugins, and one that's new since the last check is toasted once.
 */
export function usePluginUpdates(plugins: PluginView[], openPlugin: (id?: string) => void) {
  const [updates, setUpdates] = useState<Record<string, string>>({});
  const [available, setAvailable] = useState<AvailablePlugin[]>([]);
  const [checking, setChecking] = useState(false);
  const latest = useRef({ plugins, openPlugin });
  latest.current = { plugins, openPlugin };
  const announced = useRef(new Set<string>());

  const check = useCallback(async (manual = false) => {
    const github = latest.current.plugins.filter((p) => p.source?.kind === "github");
    if (manual) setChecking(true);
    const offered = api.pluginsAvailable().then(
      (list) => {
        setAvailable(list);
        return list;
      },
      () => null,
    );
    let failed = 0;
    const found: { plugin: PluginView; version: string }[] = [];
    const next: Record<string, string> = {};
    await Promise.all(
      github.map(async (plugin) => {
        try {
          const version = await api.pluginLatestVersion(plugin.id);
          next[plugin.id] = version ?? "";
          const newer = newerVersion(plugin, version ?? undefined);
          if (newer) found.push({ plugin, version: newer });
        } catch {
          failed++;
        }
      }),
    );
    setUpdates((all) => ({ ...all, ...next }));
    const list = await offered;
    if (list) announceNew(list, latest.current.plugins, () => latest.current.openPlugin());
    if (manual) setChecking(false);

    if (manual && github.length > 0 && failed === github.length) {
      toast.error("Couldn't check for plugin updates");
    } else if (manual && found.length === 0) {
      toast.success("All plugins are up to date", { duration: 4000 });
    }
    if (manual) return;
    const fresh = found.filter(({ plugin, version }) => !announced.current.has(`${plugin.id}@${version}`));
    if (fresh.length === 0) return;
    for (const { plugin, version } of fresh) announced.current.add(`${plugin.id}@${version}`);
    const open = () => latest.current.openPlugin(fresh[0].plugin.id);
    notificationSound();
    toast(fresh.length === 1 ? `${fresh[0].plugin.name} ${fresh[0].version} is available` : "Plugin updates available", {
      id: TOAST_ID,
      duration: Infinity,
      description: fresh.length === 1 ? undefined : fresh.map(({ plugin, version }) => `${plugin.name} ${version}`).join(", "),
      action: { label: "Review", onClick: open },
    });
  }, []);

  useEffect(() => {
    const first = setTimeout(() => void check(), FIRST_CHECK_MS);
    const timer = setInterval(() => void check(), CHECK_EVERY_MS);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [check]);

  return { updates, available, checking, check };
}
