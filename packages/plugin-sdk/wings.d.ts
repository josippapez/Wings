// Types for the Wings plugin SDK, API 1. `window.wings` exists in every plugin; this file only helps your editor.
// In TypeScript, add "wings-plugin-sdk" to `types` in tsconfig.json. In JavaScript, start a file with
// `// @ts-check` and `/// <reference types="wings-plugin-sdk" />`.

type Tone = "neutral" | "info" | "success" | "warning" | "danger" | "merged";

interface WingsPane {
  paneId: string;
  /** Follows `cd` in the pane. */
  cwd: string;
  /** What runs in the pane right now, like `zsh` or `claude`. */
  command: string;
  project: string;
  /** The id of that project, the same as `project().id`. Needs Wings 0.1.22. */
  projectId: string;
  /** Set while Claude Code runs in the pane. `startedAt` is when that `claude` started, in ms; a resumed session's transcript also has what came before. */
  session: { sessionId: string; name: string | null; state: string; startedAt: number } | null;
  /** The focused pane of the tab on screen. At most one pane has it, and none while no tab is open. */
  focused: boolean;
}

interface WingsTranscriptEvent {
  sessionId: string;
  /** The pane the session runs in. */
  paneId: string;
  /** The entry as Claude Code wrote it. Its `type` is one of `permissions.transcript`. */
  entry: { type: string } & Record<string, unknown>;
}

interface WingsBadge {
  label: string;
  tone: Tone;
  icon?: "pr-open" | "pr-merged" | "pr-closed" | "pr-draft";
  /** Small icon-and-number chips after the label. Leave out zeros. */
  counts?: { icon: "changed" | "push" | "pull"; value: number }[];
  /** Shows a spinner in the pill. */
  loading?: boolean;
  title?: string;
  subtitle?: string;
  rows?: { label: string; value: string; tone?: Tone }[];
  actions?: { id: string; label: string; primary?: boolean }[];
}

interface WingsComment {
  id: number;
  replyTo: number | null;
  /** `null` for a comment on the whole change. */
  path: string | null;
  /** `null` when the comment no longer matches the diff. */
  line: number | null;
  side: "additions" | "deletions";
  author: string;
  body: string;
  createdAt: string;
  url: string;
  /** On the first comment of a thread: the viewer offers a reply box, and sends replies to `onReply`. */
  canReply?: boolean;
}

/** A reply typed in the diff viewer. `replyTo` is the id of the thread's first comment. */
interface WingsReply {
  diffId: string;
  replyTo: number;
  body: string;
}

/** A usage limit window. `resetsAt` is in Unix epoch seconds. */
interface WingsLimitWindow {
  usedPercentage: number;
  resetsAt: number | null;
}

/** What Claude Code last told `wings statusline`. `at` is when Wings got it, in ms. */
interface WingsStatusline {
  /** Your account's 5-hour and weekly limits, from the newest session that reported them. `null` until one has. */
  rateLimits: { fiveHour: WingsLimitWindow | null; sevenDay: WingsLimitWindow | null; at: number } | null;
  /** By session id. Token counts are from the session's last API response. */
  sessions: Record<
    string,
    {
      /** Like `Opus 5.5 (1M context)`. */
      model: string | null;
      contextWindowSize: number | null;
      usedPercentage: number | null;
      totalInputTokens: number | null;
      totalOutputTokens: number | null;
      currentUsage: { inputTokens: number; outputTokens: number; cacheCreationInputTokens: number; cacheReadInputTokens: number } | null;
      /** `expiresAt` is in Unix epoch seconds. */
      promptCache: { warm: boolean; ttl: "5m" | "1h" | null; expiresAt: number | null } | null;
      at: number;
    }
  >;
}

interface WingsExecOptions {
  cwd?: string;
  /** 30 s by default, 5 min at most. */
  timeoutMs?: number;
  /** Gets each stdout and stderr line while the program runs. */
  onOutput?: (line: string) => void;
}

/** A project (space) as pages see it. */
interface WingsProject {
  id: string;
  name: string;
  path: string;
  branch: string | null;
}

interface Wings {
  // Every `on*` call returns a function that removes the listener, for cleaning up in a React effect.
  /** Every terminal pane, on every change. */
  onPanes(listener: (panes: WingsPane[]) => void): () => void;
  /**
   * Whether the user is in Wings: true while any Wings window (the main one or a view window) is in front, false
   * while another app is. Called once with the current state, then on each change. Needs no permission. Pause timers
   * while it is false and check once when it turns true, or use `pollWhileFocused` from `wings-plugin-sdk/poll`.
   */
  onFocus(listener: (event: { focused: boolean }) => void): () => void;
  /** The state `onFocus` last reported. True until Wings says otherwise, which it does as soon as the frame is ready. */
  readonly focused: boolean;
  /**
   * A badge action was clicked. Return a promise: the button spins until it settles (up to 5 min), and a thrown
   * error shows in the card. What it resolves to is ignored.
   */
  onAction(listener: (action: { paneId: string; actionId: string }) => unknown): () => void;
  /**
   * Handles Claude's calls to a tool in `contributes.mcpTools`. Return a string or a JSON value; a thrown error
   * is sent back as a failed call. `paneId` is the Wings pane Claude runs in, or `null`. Register it in `main`.
   */
  onTool(name: string, handler: (input: Record<string, unknown>, context: { paneId: string | null }) => unknown): void;
  /** Runs a command from `permissions.exec`, like `git branch --show-current`. */
  exec(program: string, args?: string[], options?: WingsExecOptions): Promise<{ code: number | null; stdout: string; stderr: string }>;
  /**
   * Needs `permissions.statusline`. What Claude Code last told its status line, once `wings statusline` is your
   * `statusLine` command: usage limits and each session's context and cache, as Claude Code reports them.
   */
  statusline(): Promise<WingsStatusline>;
  /** Claude Code transcript entries of types in `permissions.transcript`, oldest first. */
  transcript(sessionId: string, types: string[], options?: { last?: number }): Promise<Record<string, unknown>[]>;
  /**
   * Each transcript entry Claude Code writes while it runs in a pane, for the types in `permissions.transcript`.
   * Only entries written after the plugin started, or after the session started in the pane; `transcript` reads
   * earlier ones. Entries over 256 KB are skipped.
   */
  onTranscript(listener: (event: WingsTranscriptEvent) => void): () => void;
  /** Needs `"badges"` in `contributes.ui`. `null` removes it. */
  setBadge(paneId: string, badge: WingsBadge | null): Promise<void>;
  /**
   * Needs `permissions.ports`. The local TCP ports the programs in your panes listen on, that `localhost`
   * reaches. About 20 ms a call, so poll slowly.
   */
  ports(): Promise<{ paneId: string; pid: number; name: string; port: number; startedAt: number }[]>;
  /** Opens an https URL, or an http one to localhost, 127.0.0.1 or [::1], that starts with a prefix in `permissions.openUrl`. */
  openUrl(url: string): Promise<void>;
  /**
   * Needs `permissions.panes`. Opens a terminal in a new tab, or a split beside the focused pane, and resolves
   * once the pane is in `onPanes`. `command`, like `"npm run dev"`, is typed into its shell and must start with
   * an entry of `permissions.panes`. `cwd` must be inside one of your projects; without it the pane opens at the
   * root of the project on screen.
   */
  openPane(options?: { command?: string; cwd?: string; placement?: "tab" | "right" | "down" }): Promise<{ paneId: string }>;
  /** Needs `permissions.panes`. Shows an open pane and moves the keyboard to it. */
  focusPane(paneId: string): Promise<void>;
  /**
   * After `exec` rejected with "<program> is not installed or not on PATH": Wings asks you, installs it in a new tab
   * and restarts the plugin once it's there. Only `az`, `gh` and `glab`, and only ones `permissions.exec` names.
   * Resolves true once the install started (or it's already installed), false if you cancelled.
   */
  installTool(program: "az" | "gh" | "glab"): Promise<boolean>;
  /** Needs `permissions.notify`. A desktop notification, up to 3 a minute. The title is cut at 64 characters, the body at 256. */
  notify(notification: { title: string; body?: string }): Promise<void>;
  /** Needs `"diff"` in `contributes.ui`. Leave out `patch` to open it loading, then call `updateDiff`. */
  openDiff(diff: { title: string; subtitle?: string; /** The repo or folder the diff is of, so Send to agent offers the agent working there first. */ cwd?: string; patch?: string; comments?: WingsComment[] }): Promise<{ id: string }>;
  /** Updates a viewer opened with `openDiff`; ignored once it's closed. `{ comments }` alone keeps the patch, like after a reply. */
  updateDiff(id: string, update: { patch: string; comments?: WingsComment[] } | { comments: WingsComment[] } | { error: string }): Promise<void>;
  /**
   * Needs `"diff"` in `contributes.ui`. A reply typed under a comment with `canReply` in the diff viewer. Post it,
   * then add it with `updateDiff`. The reply box waits for the returned promise, and a thrown error shows under it
   * with the text kept. One handler: a second call replaces the first.
   */
  onReply(handler: (reply: WingsReply) => unknown): void;
  /**
   * POSTs `fields` (string or whole-number values, up to 64 KB) as JSON to a `permissions.post` path, as you:
   * `gh api`, `glab api` against `host`, or `az rest` for Azure DevOps. Wings builds the command itself.
   */
  post(
    program: "gh" | "glab" | "az",
    url: string,
    fields: Record<string, string | number>,
    options?: { host?: string },
  ): Promise<{ code: number | null; stdout: string; stderr: string }>;
  /**
   * An HTTP request to a URL under `permissions.fetch`, made by Wings. `bearer` names a secret that Wings sends
   * as `Authorization: Bearer <secret>`. Resolves for any status; redirects aren't followed.
   */
  fetch(
    url: string,
    init?: { method?: string; headers?: Record<string, string>; body?: string; bearer?: string },
  ): Promise<{ status: number; contentType: string | null; body: string }>;
  /** Secrets, like API tokens, in the system keychain. `fetch` can send them, but they can't be read back. */
  secrets: {
    set(name: string, value: string): Promise<void>;
    delete(name: string): Promise<void>;
    has(name: string): Promise<boolean>;
  };
  /**
   * The plugin's own storage for settings and state, since `localStorage` throws in plugin pages. Values are
   * JSON. Keys are 1 to 128 letters, digits and `- _ . : /`, and it holds 1 MB in all. Only this plugin sees it.
   */
  storage: {
    /** The value, or `null` when the key isn't set. */
    get(key: string): Promise<unknown>;
    /** Refused if it would take the plugin's storage over 1 MB. */
    set(key: string, value: unknown): Promise<void>;
    delete(key: string): Promise<void>;
    /** Every key, sorted. */
    keys(): Promise<string[]>;
  };
  /** Short text next to a sidebar's or view's title bar button, like a running timer. `null` clears it. */
  setSidebarLabel(sidebarId: string, label: string | null, options?: { rows?: { label: string; value: string }[]; tone?: "warning" | "danger" }): Promise<void>;
  /**
   * Opens one of `contributes.sidebars` or `contributes.views` by id: in the right sidebar, in a modal over Wings,
   * in a window of its own or as a tab in the project on screen (`tab` needs Wings 0.1.15). `as` must be in the
   * view's `opens`, and defaults to the first. A window or tab that's already open comes to the front. The page
   * gets the same `wings` object wherever it opens.
   *
   * `args` (JSON, up to 16 KB, only with `as: "tab"`) is what the tab's page reads with `viewArgs`. A tab that's
   * already open with other args takes the new ones, drops its saved tab state and reloads; a view with `"multiple": true` always opens a
   * new tab. Needs Wings 0.1.22.
   */
  openView(id: string, options?: { as?: "sidebar" | "modal" | "window" | "tab"; args?: unknown }): Promise<void>;
  /** Where this page shows. `null` in the plugin's main script. Needs Wings 0.1.22. */
  placement(): "sidebar" | "tab" | "modal" | "window" | null;
  /** What `openView` passed as `args` when it opened this tab, or `null`. Available at once. Needs Wings 0.1.22. */
  viewArgs<T = unknown>(): T | null;
  /**
   * The project this page is about, or `null` when none is open. A tab page's project is the one the tab belongs to
   * and never changes; any other page gets the project on screen. Needs Wings 0.1.22.
   */
  project(): Promise<WingsProject | null>;
  /**
   * Calls the listener with the current project, then each time it changes: the project on screen switched (not for a
   * tab page), or its branch or name changed. Needs Wings 0.1.22.
   */
  onProject(listener: (project: WingsProject | null) => void): () => void;
  /** In a tab page: the state it last saved with `setTabState`, or `null`. Outside a tab it is `null`. Opening the tab with new `args` clears it. Needs Wings 0.1.22. */
  tabState<T = unknown>(): Promise<T | null>;
  /**
   * In a tab page: saves JSON state, up to 64 KB, on the tab. Wings keeps it across restarts and while the page is
   * unloaded. Rejects outside a tab or when the state is too big. Needs Wings 0.1.22.
   */
  setTabState(state: unknown): Promise<void>;
  /** Closes a sidebar or view wherever it's open. */
  closeView(id: string): Promise<void>;
  /** Opens Wings Settings at the plugin's settings page, `contributes.settings`. Wings 0.1.15 and newer. */
  openSettings(): Promise<void>;
  /** Sends a JSON value, up to 64 KB, to the plugin's other pages: its main script and open sidebars, modals and windows. */
  broadcast(message: unknown): Promise<void>;
  /** Gets what the plugin's other pages send with `broadcast`. */
  onBroadcast(listener: (message: unknown) => void): () => void;
}

declare const wings: Wings;

/** A page's own CSS, `import "./sidebar.css"`: wings-plugin build bundles it into the page's `<name>.css`. */
declare module "*.css";
