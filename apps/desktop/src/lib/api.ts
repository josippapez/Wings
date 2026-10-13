import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { paneInput } from "./pane-input.ts";

export type Space = {
  id: string;
  name: string;
  path: string;
  branch: string | null;
  /** The project this is a git worktree of, when Wings made it for a new agent. */
  parent?: string;
};

/** An editor found on this computer. */
export type Editor = { id: string; name: string };
/** What the new-agent-in-a-worktree dialog offers for a project. `defaultBranch` is a ref like `origin/main`. */
export type WorktreeInfo = { defaultBranch: string | null; branches: string[] };

export type AgentState = "working" | "blocked" | "done" | "idle";

/** Which agent CLI runs in the pane. */
export type AgentKind = "claude" | "opencode";

export type Agent = {
  paneId: string;
  spaceId: string;
  pid: number;
  kind: AgentKind;
  sessionId: string | null;
  name: string | null;
  state: AgentState;
  waitingFor: string | null;
  /** The flags `claude` was started with, minus the ones that choose the session and any prompt. */
  args: string[];
  /** When this `claude` process started, in ms since 1970. */
  startedAt: number;
  /** When `state` last changed, in ms since 1970; null when Claude Code doesn't say. */
  stateSince: number | null;
  /** True when the pane's own shell started the agent, with nothing wrapped around it. */
  direct: boolean;
};

/** Commits to push and pull (as of the last fetch) and changed files, per project. */
export type GitStatus = { branch: string | null; ahead: number; behind: number; changed: number };

/** What a pane runs right now and where: the shell's cwd follows `cd`. */
export type PaneInfo = { command: string; cwd: string | null };

/** A pane still running in the Rust core, which a reloaded UI can attach to again. */
export type LivePane = { id: string; spaceId: string; cwd: string };

export type PluginPermissions = {
  exec: string[];
  transcript: string[];
  openUrl: string[];
  fetch: string[];
  /** API paths it may POST to through gh, glab or az; `*` stands for one path segment. */
  post: string[];
  /** Commands it may start in a new pane; `[]` is plain shells only, `null` no panes and no focusing them. */
  panes: string[] | null;
  notify: boolean;
  /** Reads what Claude Code told `wings statusline`: usage limits and each session's context and cache. */
  statusline: boolean;
  /** Sees which local ports the programs in each pane listen on. */
  ports: boolean;
};
export type PluginSource = { kind: "file" } | { kind: "github"; repo: string; folder?: string };
/** A web page a plugin shows in a popover from a title bar button. */
export type PluginPanel = { id: string; title: string; icon: string; url: string; width: number | null; height: number | null };
/** A sidebar's smaller page, shown under its title bar button while you point at it. Sizes are in CSS pixels. */
export type PluginPopover = { page: string; width: number; height: number };
/** One of the plugin's own pages, shown in the right sidebar from a title bar button. */
export type PluginSidebar = { id: string; title: string; icon: string; page: string; popover?: PluginPopover };
export type ViewPlace = "sidebar" | "modal" | "window" | "tab";
/** One of the plugin's own pages, opened in the first of `opens` from its title bar button. Sizes are in CSS pixels. An empty `icon` means the plugin's own. */
export type PluginPage = { id: string; title: string; icon: string; page: string; opens: ViewPlace[]; width: number | null; height: number | null;
  /** Listed in the + menu and the command palette; needs `tab` in `opens`. */
  newTab?: boolean;
  /** Opening it as a tab always opens a new one, instead of showing the project's existing tab. */
  multiple?: boolean;
};
/** What a plugin adds: UI it draws (`badges`, `diff`), tools it offers Claude over MCP, panels, sidebars, views and a settings page. */
export type PluginContributes = {
  ui: string[];
  mcpTools: { name: string; description: string; inputSchema: Record<string, unknown> }[];
  panels: PluginPanel[];
  sidebars: PluginSidebar[];
  views: PluginPage[];
  /** Its page in Wings Settings, under Plugin settings. Absent when it has none. */
  settings?: { page: string };
};

/** Every page of a plugin: its sidebars, which only open in the sidebar, then its views. Ids are unique across both. */
export const pagesOf = (plugin: PluginView): (PluginPage & { iconSvg: string | null })[] =>
  [
    ...plugin.contributes.sidebars.map((s) => ({ ...s, opens: ["sidebar"] satisfies ViewPlace[], width: null, height: null })),
    ...plugin.contributes.views,
  ].map((page) => ({ ...page, iconSvg: plugin.iconSvg }));
/**
 * A command-line tool a plugin runs, like the GitHub CLI. `command` is the Homebrew line Wings runs to install it:
 * set only when it's missing, on macOS, with Homebrew there. `needsHomebrew` is missing on macOS without Homebrew.
 */
export type ToolStatus = { program: string; name: string; installed: boolean; command: string | null; needsHomebrew: boolean; docs: string };
/** macOS kept another app's data from a program Wings ran, and won't ask you about it. */
export type PrivacyBlocked = { folder: string; name: string; program: string };
export type McpStatus = { claude: boolean; connected: boolean; statusline: boolean };
/** The `wings` terminal command. `available` is false in dev builds and on Windows. */
export type CliStatus = { available: boolean; installed: boolean; onPath: boolean; asked: boolean };
export type McpCall = { callId: number; pluginId: string; tool: string; arguments: Record<string, unknown>; paneId: string | null };
/** Claude asked Wings (the `open_pane` tool) for a new tab. Answer with `mcpToolResult`: the pane id, or the error. */
export type McpOpenPane = { callId: number; spaceId: string; cwd: string | null; input: string | null };
/** A transcript entry Claude just wrote in a pane. Only `plugins`, which may read its type, get it. */
export type PluginTranscriptEvent = { sessionId: string; paneId: string; entry: Record<string, unknown>; plugins: string[] };
/** A plugin as the manager shows it. It runs only while `enabled` and `approved` are both true. */
export type PluginView = {
  id: string;
  name: string;
  version: string;
  description: string;
  main: string;
  /** The checked markup of its `icon` file. Draw it with `PluginGlyph`, never as page content. */
  iconSvg: string | null;
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

/** Where a search matched: a prompt, Claude's reply, or a file the session edited. */
export type HistorySnippet = { role: "user" | "assistant" | "file"; text: string };
/** A past session from the history search. `cwd` is where it started, so where it resumes. */
export type HistoryHit = {
  id: string;
  cwd: string | null;
  title: string | null;
  firstPrompt: string | null;
  gitBranch: string | null;
  lastActiveMs: number;
  lastActive: string | null;
  messages: number;
  /** Messages that match the search. */
  matches: number;
  snippets: HistorySnippet[];
};
/** `project` also covers folders inside it, like worktrees. Leave it out to search every project. */
export type HistoryFilter = { project?: string | null; branch?: string | null; sinceMs?: number | null };
export type HistoryResults = { sessions: HistoryHit[]; total: number; branches: string[] };

export const api = {
  spacesList: () => invoke<Space[]>("spaces_list"),
  pluginsList: () => invoke<PluginView[]>("plugins_list"),
  pluginInstallFile: (path: string) => invoke<PluginView>("plugin_install_file", { path }),
  pluginInstallGithub: (url: string) => invoke<PluginView>("plugin_install_github", { url }),
  pluginUpdate: (id: string) => invoke<PluginView>("plugin_update", { id }),
  pluginLatestVersion: (id: string) => invoke<string | null>("plugin_latest_version", { id }),
  /** The command-line tools the plugin runs that Wings can install, and whether each is installed. */
  pluginTools: (id: string) => invoke<ToolStatus[]>("plugin_tools", { id }),
  /** The plugin's README.md, if it has one. */
  pluginReadme: (id: string) => invoke<string | null>("plugin_readme", { id }),
  /** Turning on approves exactly what `plugin` shows, and fails if the installed plugin changed since. */
  pluginSetEnabled: (plugin: PluginView, enabled: boolean) =>
    invoke<PluginView>("plugin_set_enabled", {
      id: plugin.id,
      enabled,
      shown: enabled ? { permissions: plugin.permissions, contributes: plugin.contributes } : null,
    }),
  pluginRemove: (id: string) => invoke<void>("plugin_remove", { id }),
  /** The `wings plugin` CLI changed a plugin. `review` means it's waiting for your approval. */
  onPluginsChanged: (cb: (change: { id: string; review: boolean }) => void): Promise<UnlistenFn> =>
    listen<{ id: string; review: boolean }>("plugins-changed", (e) => cb(e.payload)),
  /** Whether any Wings window is the key window, sent on every window focus change. */
  onWingsFocus: (cb: (focused: boolean) => void): Promise<UnlistenFn> => listen<boolean>("wings-focus", (e) => cb(e.payload)),
  /** Whether Claude Code is installed, and has the Wings MCP server that serves plugin tools. */
  mcpStatus: () => invoke<McpStatus>("mcp_status"),
  /** Registers the Wings MCP server with Claude Code, for every project. */
  mcpConnect: () => invoke<void>("mcp_connect"),
  mcpDisconnect: () => invoke<void>("mcp_disconnect"),
  privacyOpenSettings: () => invoke<void>("privacy_open_settings"),
  privacyCheck: (folder: string) => invoke<boolean>("privacy_check", { folder }),
  onPrivacyBlocked: (cb: (blocked: PrivacyBlocked) => void): Promise<UnlistenFn> =>
    listen<PrivacyBlocked>("privacy-blocked", (e) => cb(e.payload)),
  cliStatus: () => invoke<CliStatus>("cli_status"),
  /** Adds the `wings` command to ~/.local/bin. */
  cliInstall: () => invoke<CliStatus>("cli_install"),
  /** "Not now" on the first-start prompt. */
  cliDismiss: () => invoke<void>("cli_dismiss"),
  mcpToolResult: (callId: number, text: string, isError: boolean) => invoke<void>("mcp_tool_result", { callId, text, isError }),
  /** Claude called a plugin tool. `paneId` is the pane that Claude runs in, if it's a Wings pane. */
  onMcpCall: (cb: (call: McpCall) => void): Promise<UnlistenFn> => listen<McpCall>("mcp-call", (e) => cb(e.payload)),
  onMcpOpenPane: (cb: (call: McpOpenPane) => void): Promise<UnlistenFn> => listen<McpOpenPane>("mcp-open-pane", (e) => cb(e.payload)),
  /** Entries Claude wrote to the transcripts of sessions in panes since the last check, about twice a second. */
  onPluginTranscript: (cb: (events: PluginTranscriptEvent[]) => void): Promise<UnlistenFn> =>
    listen<PluginTranscriptEvent[]>("plugin-transcript", (e) => cb(e.payload)),
  /** `right` and `bottom` are the button's edges in the window, so the panel opens just under it. */
  pluginPanelToggle: (pluginId: string, panelId: string, right: number, bottom: number) =>
    invoke<void>("plugin_panel_toggle", { pluginId, panelId, right, bottom }),
  spacesAdd: (path: string) => invoke<Space>("spaces_add", { path }),
  spacesRemove: (id: string) => invoke<void>("spaces_remove", { id }),
  worktreeInfo: (spaceId: string) => invoke<WorktreeInfo>("worktree_info", { spaceId }),
  /** `git worktree add -b <branch> <folder beside the repo> <base>`, added to the sidebar under the project. Rejects with git's message. */
  worktreeCreate: (spaceId: string, branch: string, base: string) => invoke<Space>("worktree_create", { spaceId, branch, base }),
  editorsList: () => invoke<Editor[]>("editors_list"),
  /** Opens the project's folder in the editor. Rejects with the editor's message. */
  editorOpen: (editorId: string, spaceId: string) => invoke<void>("editor_open", { editorId, spaceId }),
  sessionsList: (spaceId: string) => invoke<SessionSummary[]>("sessions_list", { spaceId }),
  /** Past Claude Code sessions, most recent first. Every word or "quoted phrase" in `query` has to match. */
  historySearch: (query: string, filter: HistoryFilter, limit: number) =>
    invoke<HistoryResults>("history_search", { query, filter, limit }),
  /** Makes `accelerator` (like "Ctrl+Alt+KeyJ") the shortcut that works from any app, or none with null. Rejects if it can't be used. */
  hotkeySet: (accelerator: string | null) => invoke<void>("hotkey_set", { accelerator }),
  /** The global shortcut was pressed; Wings is already in front. */
  onGlobalHotkey: (cb: () => void): Promise<UnlistenFn> => listen("global-hotkey", () => cb()),
  /** Whether an agent process has a child doing work: a Bash command still running, possibly in the background. */
  agentBusy: (pid: number) => invoke<boolean>("agent_busy", { pid }),
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
    /** A folder inside the project to start in, instead of its root. */
    cwd: string | null = null,
  ) => invoke<string>("pane_create", { spaceId, cols, rows, initialInput, onOutput, cwd }),
  /** Keys you type (the terminal's input). Remembered per pane, so Wings can tell a draft is waiting in one. */
  paneWrite: (id: string, data: string) => {
    paneInput.note(id, data);
    return invoke<void>("pane_write", { id, data });
  },
  /** Input Wings sends itself, like a resume line: it is no draft of yours. */
  paneSend: (id: string, data: string) => invoke<void>("pane_write", { id, data }),
  paneResize: (id: string, cols: number, rows: number) => invoke<void>("pane_resize", { id, cols, rows }),
  paneClose: (id: string) => invoke<void>("pane_close", { id }),
  panesList: () => invoke<LivePane[]>("panes_list"),
  /** Streams a running pane to `onOutput`, starting with the output it kept. */
  paneAttach: (id: string, onOutput: Channel<ArrayBuffer>) => invoke<void>("pane_attach", { id, onOutput }),
  /** Kills every pane but `keep`, the ones this UI attaches to again. */
  panesReset: (keep: string[]) => invoke<void>("panes_reset", { keep }),
  workspaceLoad: () => invoke<string | null>("workspace_load"),
  workspaceSave: (json: string) => invoke<void>("workspace_save", { json }),
  paneFocus: (id: string | null) => invoke<void>("pane_focus", { id }),
  onAgents: (cb: (agents: Agent[]) => void): Promise<UnlistenFn> => listen<Agent[]>("agents", (e) => cb(e.payload)),
  /** A system notification was clicked: go to that pane. */
  onFocusPane: (cb: (paneId: string) => void): Promise<UnlistenFn> => listen<string>("focus-pane", (e) => cb(e.payload)),
  onMenu: (cb: (id: string) => void): Promise<UnlistenFn> => listen<string>("menu", (e) => cb(e.payload)),
  onPaneExited: (cb: (paneId: string) => void): Promise<UnlistenFn> =>
    listen<string>("pane-exited", (e) => cb(e.payload)),
};
