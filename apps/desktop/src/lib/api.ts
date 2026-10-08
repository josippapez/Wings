import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Space = { id: string; name: string; path: string; branch: string | null };

export type AgentState = "working" | "blocked" | "done" | "idle";

export type Agent = {
  paneId: string;
  spaceId: string;
  pid: number;
  sessionId: string | null;
  name: string | null;
  state: AgentState;
  waitingFor: string | null;
};

/** Commits to push and pull (as of the last fetch) and changed files, per project. */
export type GitStatus = { branch: string | null; ahead: number; behind: number; changed: number };

/** What a pane runs right now and where: the shell's cwd follows `cd`. */
export type PaneInfo = { command: string; cwd: string | null };

export type PluginPermissions = { exec: string[]; transcript: string[]; openUrl: string[] };
export type PluginSource = { kind: "file" } | { kind: "github"; repo: string };
/** What a plugin adds: UI it draws (`badges`, `diff`) and tools it offers Claude over MCP. */
export type PluginContributes = { ui: string[]; mcpTools: { name: string; description: string }[] };
/** A plugin as the manager shows it. It runs only while `enabled` and `approved` are both true. */
export type PluginView = {
  id: string;
  name: string;
  version: string;
  description: string;
  main: string;
  permissions: PluginPermissions;
  contributes: PluginContributes;
  enabled: boolean;
  /** False until you approve this version's permissions, and again after an update changes them. */
  approved: boolean;
  source: PluginSource | null;
  /** Loaded from the repo by a dev build: always approved, can't be removed. */
  dev: boolean;
};

export type SessionSummary = {
  id: string;
  title: string | null;
  firstPrompt: string | null;
  gitBranch: string | null;
  lastActiveMs: number;
  sizeBytes: number;
};

export const api = {
  spacesList: () => invoke<Space[]>("spaces_list"),
  pluginsList: () => invoke<PluginView[]>("plugins_list"),
  pluginInstallFile: (path: string) => invoke<PluginView>("plugin_install_file", { path }),
  pluginInstallGithub: (url: string) => invoke<PluginView>("plugin_install_github", { url }),
  pluginUpdate: (id: string) => invoke<PluginView>("plugin_update", { id }),
  pluginLatestVersion: (id: string) => invoke<string | null>("plugin_latest_version", { id }),
  /** Turning on approves exactly what `plugin` shows, and fails if the installed plugin changed since. */
  pluginSetEnabled: (plugin: PluginView, enabled: boolean) =>
    invoke<PluginView>("plugin_set_enabled", {
      id: plugin.id,
      enabled,
      shown: enabled ? { permissions: plugin.permissions, contributes: plugin.contributes } : null,
    }),
  pluginRemove: (id: string) => invoke<void>("plugin_remove", { id }),
  spacesAdd: (path: string) => invoke<Space>("spaces_add", { path }),
  spacesRemove: (id: string) => invoke<void>("spaces_remove", { id }),
  sessionsList: (spaceId: string) => invoke<SessionSummary[]>("sessions_list", { spaceId }),
  agentsList: () => invoke<Agent[]>("agents_list"),
  gitStatus: () => invoke<Record<string, GitStatus>>("git_status"),
  gitRefresh: () => invoke<Record<string, GitStatus>>("git_refresh"),
  onGitStatus: (cb: (status: Record<string, GitStatus>) => void): Promise<UnlistenFn> =>
    listen<Record<string, GitStatus>>("git-status", (e) => cb(e.payload)),
  paneInfo: () => invoke<Record<string, PaneInfo>>("pane_info"),
  onPaneInfo: (cb: (info: Record<string, PaneInfo>) => void): Promise<UnlistenFn> =>
    listen<Record<string, PaneInfo>>("pane-info", (e) => cb(e.payload)),
  paneCreate: (
    spaceId: string,
    cols: number,
    rows: number,
    initialInput: string | null,
    onOutput: Channel<ArrayBuffer>,
  ) => invoke<string>("pane_create", { spaceId, cols, rows, initialInput, onOutput }),
  paneWrite: (id: string, data: string) => invoke<void>("pane_write", { id, data }),
  paneResize: (id: string, cols: number, rows: number) => invoke<void>("pane_resize", { id, cols, rows }),
  paneClose: (id: string) => invoke<void>("pane_close", { id }),
  panesReset: () => invoke<void>("panes_reset"),
  workspaceLoad: () => invoke<string | null>("workspace_load"),
  workspaceSave: (json: string) => invoke<void>("workspace_save", { json }),
  paneFocus: (id: string | null) => invoke<void>("pane_focus", { id }),
  onAgents: (cb: (agents: Agent[]) => void): Promise<UnlistenFn> => listen<Agent[]>("agents", (e) => cb(e.payload)),
  onMenu: (cb: (id: string) => void): Promise<UnlistenFn> => listen<string>("menu", (e) => cb(e.payload)),
  onPaneExited: (cb: (paneId: string) => void): Promise<UnlistenFn> =>
    listen<string>("pane-exited", (e) => cb(e.payload)),
};
