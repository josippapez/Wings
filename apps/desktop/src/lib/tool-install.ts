import type { PaneInfo, PluginView, ToolStatus } from "./api.ts";
import { isShellCommand } from "./resume.ts";

/**
 * Installing a command-line tool a plugin runs, like the GitHub CLI. Rust says which tools a plugin needs and gives
 * the install command from its own table; Wings asks you first, then types that command into a new tab.
 */

export type ToolRequest = { pluginId: string; pluginName: string; tool: ToolStatus };

let handler: ((request: ToolRequest) => Promise<boolean>) | null = null;

/** The confirmation dialog takes requests while it's mounted. */
export function onToolInstall(next: (request: ToolRequest) => Promise<boolean>) {
  handler = next;
  return () => {
    if (handler === next) handler = null;
  };
}

/** Asks you to install `tool`. Resolves true once the install started in a tab, false if you said no. */
export function requestToolInstall(request: ToolRequest): Promise<boolean> {
  if (!handler) return Promise.reject(new Error("Wings can't install tools right now"));
  return handler(request);
}

/** The plugins that run `program`, so they start again once it's installed. */
export const pluginsUsing = (plugins: PluginView[], program: string) =>
  plugins.filter((p) => [...p.permissions.exec, ...p.permissions.post].some((e) => e.split(/\s+/)[0] === program)).map((p) => p.id);

/** A tab running an install. `seen` once the pane shows up in pane info, `ran` once a program other than the shell ran in it. */
export type InstallWatch = ToolRequest & { paneId: string; seen: boolean; ran: boolean };

/**
 * After pane info changed: an install is over when its command went back to the shell prompt, or its tab closed.
 * Returns the watches still running, updated, and the finished ones.
 */
export function settleInstalls(watches: InstallWatch[], info: Record<string, PaneInfo>) {
  const running: InstallWatch[] = [];
  const done: InstallWatch[] = [];
  for (const watch of watches) {
    const pane = info[watch.paneId];
    const ran = watch.ran || (pane !== undefined && !isShellCommand(pane.command));
    if ((watch.seen && !pane) || (ran && isShellCommand(pane?.command))) done.push(watch);
    else running.push({ ...watch, seen: watch.seen || pane !== undefined, ran });
  }
  return { running, done };
}
