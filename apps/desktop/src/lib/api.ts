import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { paneInput } from "./pane-input.ts";
import type { SessionKind } from "./resume.ts";

export type Space = {
  id: string;
  name: string;
  path: string;
  branch: string | null;
  /** The project this is a git worktree of, when Wings made it for a new agent. */
  parent?: string;
  /** When the project was added, in ms. Absent for projects added before Wings kept it. */
  addedAt?: number;
  /** Listed above the unpinned projects. Absent when not pinned. */
  pinned?: boolean;
};

/** An editor found on this computer. */
export type Editor = { id: string; name: string };
/** What the new-agent-in-a-worktree dialog offers for a project. `defaultBranch` is a ref like `origin/main`. */
export type WorktreeInfo = { defaultBranch: string | null; branches: string[] };

/** A worktree Wings made: its project, the gitignored local files copied into it, and the project's setup line to run there first. */
export type WorktreeMade = { space: Space; local: { copied: number; skipped: number; errors: string[] }; setup: string | null };
/** What a project's worktrees get. `include` and `setup` are null when unset; `hasFile` is a `.worktreeinclude` in the repo, used instead of `include`. */
export type WorktreeLocal = { include: string | null; setup: string | null; defaultInclude: string; hasFile: boolean };

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
  /** One line on what the agent just did, made when its turn ended; null while Wings has none or the model is off. */
  summary: string | null;
};

/** Which model makes session names and summaries. */
export type AiEngine = "apple" | "ollama" | "off";
export type AiStatus = {
  engine: AiEngine;
  ollamaModel: string | null;
  available: boolean;
  /** Why the engine can't run right now, for the setting to show. */
  reason: string | null;
  /** Installed Ollama models, when Ollama is chosen and running. */
  models: string[];
};

/** Commits to push and pull (as of the last fetch) and changed files, per project. */
export type GitStatus = { branch: string | null; ahead: number; behind: number; changed: number };

/** What a pane runs right now and where: the shell's cwd follows `cd`. */
export type PaneInfo = { command: string; cwd: string | null; busy: boolean };

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
  /** Reads what an OpenCode session is doing. */
  agentActivity: boolean;
  /** Reads the names and sizes of the files and folders on your disks, and lists the disks. */
  diskUsage: boolean;
  /** Types messages into the prompts of the agents in your panes. */
  sendToAgents: boolean;
  /** Makes git worktrees of your projects. */
  worktrees: boolean;
  /** Other plugins' tools it calls: a plugin id, or `<plugin> <tool>`. Only between Wings' own plugins. */
  callTools: string[];
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
  /** Works with no project on screen: the + menu opens it as a tab then too; needs `tab` in `opens`. */
  noProject?: boolean;
  /** `false`: no title bar button, for a page that shows one item opened from a row. */
  button?: boolean;
};
/** A list a plugin shows under each project in the sidebar, filled with `wings.setProjectSection`. An empty `icon` means the plugin's own. */
/** `all` is a view that opens as a tab and lists everything: the section's All button opens it. */
export type PluginProjectSection = { id: string; title: string; icon: string; all?: string; sorts?: { id: string; label: string }[] };
/** A page of the plugin the user can place on a Project page. `minHeight` is in CSS pixels, 80 to 800; absent means 160. An empty `icon` means the plugin's own. */
export type PluginBlock = { id: string; title: string; icon: string; page: string; minHeight?: number };
/** What a plugin adds: UI it draws (`badges`, `diff`), tools it offers Claude over MCP, panels, sidebars, views and a settings page. */
export type PluginContributes = {
  ui: string[];
  mcpTools: { name: string; description: string; inputSchema: Record<string, unknown> }[];
  /** A line telling Claude when to use `mcpTools`, added to the Wings MCP server's instructions; absent when none. */
  mcpInstructions?: string;
  /** Claude Code hooks it answers with `wings.onHook`; absent when none. */
  hooks?: ("SessionStart" | "PreToolUse" | "UserPromptSubmit" | "Journal")[];
  panels: PluginPanel[];
  sidebars: PluginSidebar[];
  views: PluginPage[];
  /** Sections it can show under each project; absent when it has none. */
  projectSections?: PluginProjectSection[];
  /** Pages it offers for the Project page; absent when it has none. */
  blocks?: PluginBlock[];
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
export type McpStatus = { claude: boolean; connected: boolean; statusline: boolean; alwaysLoad: boolean };
/** A coding agent CLI: whether it's on this Mac and signed in. `login` and `install` come from Wings' own table. */
export type HarnessStatus = {
  id: string;
  name: string;
  installed: boolean;
  version: string | null;
  auth: "signedIn" | "signedOut" | "unknown";
  account: string | null;
  /** Why the sign-in state is unknown, when the CLI said. */
  note: string | null;
  login: string;
  install: string;
  docs: string;
};
/** Whether OpenCode is installed, and its global config has the Wings MCP server. */
export type OpencodeMcpStatus = { opencode: boolean; connected: boolean };
/** The `wings` terminal command. `available` is false in dev builds and on Windows. */
export type CliStatus = { available: boolean; installed: boolean; onPath: boolean; asked: boolean };
/** A tool call from Claude, and who made it: the Wings pane it runs in, that pane's project and the Claude session there. Each is null when there is none. */
export type McpCall = {
  callId: number;
  pluginId: string;
  /** The tool's name, or for a hook the event's name. */
  tool: string;
  /** Set when `tool` is a Claude Code hook event and `arguments` the hook's input. */
  hook?: boolean;
  arguments: Record<string, unknown>;
  paneId: string | null;
  project: { id: string; name: string; path: string } | null;
  session: { id: string; name: string | null } | null;
};
/** Claude asked Wings (the `open_pane` tool) for a new tab. Answer with `mcpToolResult`: the pane id, or the error. */
/** `run`: Claude asked for Enter after `input`, which the window presses only while the user allows it. */
export type McpOpenPane = { callId: number; spaceId: string; cwd: string | null; input: string | null; run: boolean; /** The calling agent's pane, to split below. */ nextTo: string | null };
/** Claude asked Wings (`close_pane`) to close a pane. Answer with `mcpToolResult`. */
export type McpClosePane = { callId: number; paneId: string };
/** Claude asked Wings (`send_to_agent`) to type into another agent's prompt. Answer with `mcpToolResult`: `{ submitted }`, or the error. */
export type McpSendToAgent = { callId: number; paneId: string; text: string };
/** A transcript entry Claude just wrote in a pane. Only `plugins`, which may read its type, get it. */
export type PluginTranscriptEvent = { sessionId: string; paneId: string; entry: Record<string, unknown>; plugins: string[] };
/** An OpenCode session in a pane did something. Only `plugins`, which may read agent activity, get it. */
export type PluginAgentActivityEvent = { sessionId: string; plugins: string[] };
/** A plugin as the manager shows it. It runs only while `enabled` and `approved` are both true. */
/** A plugin in the Wings repo that Settings offers to install. `link` is what `pluginInstallGithub` takes. */
export type AvailablePlugin = { id: string; name: string; description: string; version: string; link: string };

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
  /** The folder a linked plugin runs from (`wings-plugin dev`); its source is still a file. */
  folder?: string;
};

export type SessionSummary = {
  id: string;
  title: string | null;
  firstPrompt: string | null;
  gitBranch: string | null;
  lastActiveMs: number;
  sizeBytes: number;
};

/** Where a search matched: a prompt, the agent's reply, or a file the session edited. */
export type HistorySnippet = { role: "user" | "assistant" | "file"; text: string };
/** A past session from the history search. `cwd` is where it started, so where it resumes. */
export type HistoryHit = {
  id: string;
  /** Which agent ran it, so which CLI resumes it. */
  agent: SessionKind;
  cwd: string | null;
  title: string | null;
  /** A name Wings generated for a session with no title, when it has made one. */
  name?: string;
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
export type HistoryResults = {
  sessions: HistoryHit[];
  total: number;
  branches: string[];
  /** OpenCode's sessions are still being fetched, so a search a moment later may find more. */
  opencodeLoading: boolean;
};

export const api = {
  spacesList: () => invoke<Space[]>("spaces_list"),
  pluginsList: () => invoke<PluginView[]>("plugins_list"),
  pluginInstallFile: (path: string) => invoke<PluginView>("plugin_install_file", { path }),
  pluginInstallGithub: (url: string) => invoke<PluginView>("plugin_install_github", { url }),
  pluginUpdate: (id: string) => invoke<PluginView>("plugin_update", { id }),
  /** A plugin notification's link button; Rust checks it against the plugin's `openUrl` permission. */
  pluginOpenUrl: (pluginId: string, url: string) => invoke<void>("plugin_open_url", { pluginId, url }),
  pluginLatestVersion: (id: string) => invoke<string | null>("plugin_latest_version", { id }),
  /** The plugins in the Wings repo that run on this Wings, installed or not. */
  pluginsAvailable: () => invoke<AvailablePlugin[]>("plugins_available"),
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
  /** Re-registers the Wings MCP server so its tools load at session start, or back behind tool search. */
  mcpSetAlwaysLoad: (on: boolean) => invoke<void>("mcp_set_always_load", { on }),
  opencodeMcpStatus: () => invoke<OpencodeMcpStatus>("opencode_mcp_status"),
  /** Runs each coding agent's CLI to see if it's installed and signed in. Takes a few seconds. */
  harnesses: () => invoke<HarnessStatus[]>("harnesses_status"),
  /** Adds the Wings MCP server to OpenCode's global config, keeping fields you set on it. */
  opencodeMcpConnect: () => invoke<void>("opencode_mcp_connect"),
  opencodeMcpDisconnect: () => invoke<void>("opencode_mcp_disconnect"),
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
  onMcpClosePane: (cb: (call: McpClosePane) => void): Promise<UnlistenFn> => listen<McpClosePane>("mcp-close-pane", (e) => cb(e.payload)),
  onMcpSendToAgent: (cb: (call: McpSendToAgent) => void): Promise<UnlistenFn> => listen<McpSendToAgent>("mcp-send-to-agent", (e) => cb(e.payload)),
  /** Entries Claude wrote to the transcripts of sessions in panes since the last check, about twice a second. */
  onPluginTranscript: (cb: (events: PluginTranscriptEvent[]) => void): Promise<UnlistenFn> =>
    listen<PluginTranscriptEvent[]>("plugin-transcript", (e) => cb(e.payload)),
  /** OpenCode sessions in panes that started, ended or did a tool call, one event per session per burst. */
  onPluginAgentActivity: (cb: (events: PluginAgentActivityEvent[]) => void): Promise<UnlistenFn> =>
    listen<PluginAgentActivityEvent[]>("plugin-agent-activity", (e) => cb(e.payload)),
  /** The ids of the projects whose repo moved (a commit, checkout, pull or ref change), once the writes settle. */
  onGitChanged: (cb: (projectIds: string[]) => void): Promise<UnlistenFn> => listen<string[]>("git-changed", (e) => cb(e.payload)),
  /** `right` and `bottom` are the button's edges in the window, so the panel opens just under it. */
  pluginPanelToggle: (pluginId: string, panelId: string, right: number, bottom: number) =>
    invoke<void>("plugin_panel_toggle", { pluginId, panelId, right, bottom }),
  spacesAdd: (path: string) => invoke<Space>("spaces_add", { path }),
  spacesRemove: (id: string) => invoke<void>("spaces_remove", { id }),
  worktreeInfo: (spaceId: string) => invoke<WorktreeInfo>("worktree_info", { spaceId }),
  /** `git worktree add -b <branch> <folder beside the repo> <base>`, added to the sidebar under the project. Rejects with git's message. */
  spacesSetPinned: (id: string, pinned: boolean) => invoke<void>("spaces_set_pinned", { id, pinned }),
  worktreeCreate: (spaceId: string, branch: string, base: string) => invoke<WorktreeMade>("worktree_create", { spaceId, branch, base }),
  worktreeLocal: (spaceId: string) => invoke<WorktreeLocal>("worktree_local", { spaceId }),
  /** Blank means unset. */
  worktreeLocalSet: (spaceId: string, include: string, setup: string) => invoke<void>("worktree_local_set", { spaceId, include, setup }),
  editorsList: () => invoke<Editor[]>("editors_list"),
  /** Opens the project's folder in the editor. Rejects with the editor's message. */
  editorOpen: (editorId: string, spaceId: string) => invoke<void>("editor_open", { editorId, spaceId }),
  sessionsList: (spaceId: string) => invoke<SessionSummary[]>("sessions_list", { spaceId }),
  /** Past Claude Code sessions, most recent first. Every word or "quoted phrase" in `query` has to match. */
  historySearch: (query: string, filter: HistoryFilter, limit: number) =>
    invoke<HistoryResults>("history_search", { query, filter, limit }),
  aiStatus: () => invoke<AiStatus>("ai_status"),
  /** Chooses the engine (and the Ollama model), then answers whether it works. */
  aiSet: (engine: AiEngine, ollamaModel: string | null) => invoke<AiStatus>("ai_set", { engine, ollamaModel }),
  /** Names Wings just generated, by session id. */
  onAiNames: (cb: (names: Record<string, string>) => void): Promise<UnlistenFn> => listen<Record<string, string>>("ai-names", (e) => cb(e.payload)),
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
    /** Names the pane's output saved across a restart: it starts with what that pane showed before. */
    historyId: string | null = null,
  ) => invoke<string>("pane_create", { spaceId, cols, rows, initialInput, onOutput, cwd, historyId }),
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
  /** Deletes saved output from before a restart that none of `keep` will show. */
  /** A pane's output saved before Wings restarted, empty when there is none. */
  historyLoad: (id: string) => invoke<ArrayBuffer>("history_load", { id }),
  historiesKeep: (keep: string[]) => invoke<void>("histories_keep", { keep }),
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
