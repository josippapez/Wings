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
  /**
   * Set while an agent runs in the pane and has a session id. `kind` says which agent: only a `"claude"` session has a
   * transcript (`kind` is Wings 0.1.60; before it, every session was Claude Code's). `startedAt` is when that agent
   * process started, in ms; a resumed session's transcript also has what came before.
   */
  session: { sessionId: string; kind: WingsAgentKind; name: string | null; state: string; startedAt: number } | null;
  /** The focused pane of the tab on screen. At most one pane has it, and none while no tab is open. */
  focused: boolean;
}

interface WingsTranscriptEvent {
  sessionId: string;
  /** The pane the session runs in. */
  paneId: string;
  /** The entry as Claude Code wrote it. Its `type` is one of `permissions.transcript`. */
  entry: TranscriptEntry;
}

/** One block of a message's `content`. Which fields it has depends on `type`: `text` has `text`, `tool_use` has `id`, `name` and `input`, `tool_result` has `tool_use_id`, `content` and `is_error`. */
interface TranscriptBlock {
  type: string;
  id?: string;
  /** The tool called, like `Bash`, or `mcp__<server>__<tool>` for an MCP tool. */
  name?: string;
  /** What Claude passed the tool. */
  input?: Record<string, unknown> | null;
  tool_use_id?: string;
  is_error?: boolean;
  /** A tool result's output: text, or a list of blocks. */
  content?: unknown;
  text?: string;
  [field: string]: unknown;
}

/** The `message` of an `assistant` or `user` entry. */
interface TranscriptMessage {
  /** Claude Code writes one assistant message as several entries, and each has the message's `id`. */
  id?: string;
  /** On an assistant message. */
  model?: string;
  /** On an assistant message: the tokens of the API response it came from. */
  usage?: { input_tokens?: number; output_tokens?: number; cache_creation_input_tokens?: number; cache_read_input_tokens?: number };
  /** What you typed is a string. Claude's reply, and the tool results in a `user` entry, are lists of blocks. */
  content?: string | TranscriptBlock[];
}

interface TranscriptEntryBase {
  /** Identifies the entry. Assistant, user and attachment entries have one. */
  uuid?: string;
  /** When Claude Code wrote it, an ISO 8601 string. */
  timestamp?: string;
  /** True on an entry from a subagent's conversation, not the session's own. */
  isSidechain?: boolean;
  /** True on a `user` entry that Claude Code wrote itself, like a caveat before a command's output, not something you typed. */
  isMeta?: boolean;
  /** Claude Code owns the rest of the entry, and it changes between versions. */
  [field: string]: unknown;
}

/** Claude's reply, one block per entry. */
interface TranscriptAssistantEntry extends TranscriptEntryBase {
  type: "assistant";
  message?: TranscriptMessage;
}

/** What you sent, or the results of Claude's tool calls. */
interface TranscriptUserEntry extends TranscriptEntryBase {
  type: "user";
  message?: TranscriptMessage;
}

interface TranscriptAttachmentEntry extends TranscriptEntryBase {
  type: "attachment";
  /** `type` is the kind, which `permissions.transcript` names as `attachment:<kind>`. The other fields depend on it: these are the ones the bundled plugins read. */
  attachment?: {
    type?: string;
    /** `model`: `modelId` is like `claude-opus-5-5`, with `[1m]` after it for the 1M-token window. */
    identity?: { modelId?: string };
    /** `deferred_tools_delta`: the tools added and removed since the last one, like `mcp__<server>__<tool>`. */
    addedNames?: string[];
    removedNames?: string[];
    /** `deferred_tools_delta`: every MCP server that has failed to connect, or is still connecting, not only what changed. */
    failedMcpServers?: { name?: string; errorCode?: string; error?: string }[];
    pendingMcpServers?: string[];
    /** `invoked_skills`. */
    skills?: { name?: string }[];
    /** `queued_command`, a message sent while Claude works: `humanTurn` is true when you typed it, and `prompt` is a string or a list of blocks. */
    humanTurn?: boolean;
    prompt?: unknown;
    [field: string]: unknown;
  };
}

/** The pull request Claude Code recorded for the session. */
interface TranscriptPrLinkEntry extends TranscriptEntryBase {
  type: "pr-link";
  prUrl?: string;
  prNumber: number;
  prRepository: string;
}

/**
 * A transcript entry, of the types plugins read so far. Claude Code writes more, and owns what is in them: to read
 * another type, compare it as a plain `string`, and take its fields from the index signature.
 */
type TranscriptEntry = TranscriptAssistantEntry | TranscriptUserEntry | TranscriptAttachmentEntry | TranscriptPrLinkEntry;

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

/** One figure in a pane's status strip. */
interface WingsStatusItem {
  /** Muted text before the value. Up to 16 characters. */
  label?: string;
  /** Up to 32 characters. */
  value: string;
  /** 0 to 100. Draws a small meter between the label and the value. */
  percent?: number;
  /** Colours the meter and the value. */
  tone?: Tone;
}

/** What a plugin shows under a pane's terminal. */
interface WingsPaneStatus {
  /** One to 4 figures, left to right. */
  items: WingsStatusItem[];
  /** Up to 12 lines in the card shown when you point at the strip. */
  rows?: { label: string; value: string; tone?: Tone }[];
  /** The id of one of the plugin's sidebars or views. Clicking the strip opens it. */
  open?: string;
}

/** One line of a pane rail. */
interface WingsRailItem {
  /** Up to 200 characters. */
  text: string;
  status: "pending" | "in_progress" | "done";
}

/** A small card floating over a pane's terminal, like an agent's todo list. */
interface WingsPaneRail {
  /** Up to 40 characters. Shown in the header, and with `done/total` when minimised. */
  title: string;
  /** Up to 30 lines. */
  items: WingsRailItem[];
  /** The id of one of the plugin's sidebars or views. Clicking the title opens it. */
  open?: string;
}

/** Where the diff viewer scrolls to and highlights once the patch is shown. A path or line the patch doesn't have just opens the diff. */
interface WingsDiffFocus {
  path: string;
  line?: number;
  /** `new` (the default) counts lines after the change, `old` before it. */
  side?: "old" | "new";
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

/** A line in a plugin's section under a project in the sidebar. Needs Wings 0.1.23. */
interface WingsProjectSectionRow {
  /** Up to 64 characters, unique in the section. */
  id: string;
  /** Up to 120 characters. */
  label: string;
  /** Muted text under the label, up to 120 characters. */
  detail?: string;
  /** A small pill after the label. `text` is up to 24 characters. */
  badge?: { text: string; tone?: Tone };
  /**
   * An icon in place of the row's dot, in the badge's tone: a pull request's (Wings 0.1.35), or a work status drawn
   * like Linear's: `todo` an empty circle, `in-progress`, `in-review`, `done` a checked circle, `canceled` (Wings 0.1.40).
   */
  icon?: "pr-open" | "pr-merged" | "pr-closed" | "pr-draft" | "todo" | "in-progress" | "in-review" | "done" | "canceled";
  /** Clicking the row opens `view`, one of the plugin's `contributes.views` that lists `tab` in `opens`, as a tab in the row's project, with `args` (JSON, up to 16 KB) as `viewArgs()`. */
  open: { view: string; args?: unknown };
}

/** The agent CLIs Wings follows in its panes. */
type WingsAgentKind = "claude" | "opencode";

/** What a tool's handler is told about who called it: the Wings pane the agent runs in, that pane's project and the agent's session. */
interface WingsToolContext {
  /** The Wings pane the agent runs in, or `null` when the call didn't come from a pane in Wings. */
  paneId: string | null;
  /** The project of that pane, or `null` when there is no pane. Needs Wings 0.1.50. */
  project: { id: string; name: string; path: string } | null;
  /**
   * The agent session running in that pane, Claude Code's or opencode's, or `null` when there is no pane or the agent
   * has no session id yet. Needs Wings 0.1.50; `kind`, and opencode sessions, need Wings 0.1.60.
   */
  session: { id: string; name: string | null; kind: WingsAgentKind } | null;
}

/** What Claude Code sends a hook, as `wings.onHook` passes it on. `SessionStart` adds `source`; `PreToolUse` adds `tool_name` (always `"Bash"`) and `tool_input`. `Journal` isn't from Claude Code: it is what an agent's change journal reports at the end of a turn through `wings journal`, and adds `agent`, `prompt`, `files`, `more` and `commits`. */
interface WingsHookInput {
  session_id: string;
  transcript_path?: string;
  cwd?: string;
  permission_mode?: string;
  hook_event_name: "SessionStart" | "PreToolUse" | "Journal";
  /** SessionStart: `"startup"`, `"resume"`, `"clear"`, `"compact"` or `"fork"`. */
  source?: string;
  /** PreToolUse. */
  tool_name?: string;
  /** PreToolUse: for Bash, `{ command, description?, ... }`. */
  tool_input?: { command?: string; [key: string]: unknown };
  /** Journal: the agent whose turn it was. */
  agent?: "claude-code" | "opencode";
  /** Journal: the first 200 characters of the turn's prompt. */
  prompt?: string;
  /** Journal: the files the turn changed, each with a one-line description. */
  files?: { path: string; what: string }[];
  /** Journal: how many more files changed than `files` lists. */
  more?: number;
  /** Journal: short hashes of the commits the turn made, oldest first. */
  commits?: string[];
  [key: string]: unknown;
}

/** What a hook handler answers: a string is `context`. `deny` only counts for `PreToolUse`. */
type WingsHookAnswer = string | { context?: string; deny?: string } | null | undefined;

interface Wings {
  // Every `on*` call returns a function that removes the listener, for cleaning up in a React effect.
  /** Every terminal pane, on every change. */
  onPanes(listener: (panes: WingsPane[]) => void): () => void;
  /**
   * Whether the user is in Wings: true while any Wings window (the main one or a view window) is in front, false
   * while another app is. In a page it is also false while the page is out of view: a tab that isn't showing, a sidebar
   * that is closed or showing another page, a popover that is closed, a block that is scrolled away. Needs Wings 0.1.50.
   * Called once with the current state, then on each change. Needs no permission. Pause timers
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
   * Handles Claude's calls to a tool in `contributes.mcpTools`. Claude doesn't read tool descriptions until it
   * searches for a tool, so add `contributes.mcpInstructions`, a line on when to use them. Return a string or a JSON value; a thrown error
   * is sent back as a failed call. `context` says who called: the Wings pane Claude runs in, that pane's project and the
   * Claude session running there, so a tool can act on "this project" without tracking panes. Register it in `main`.
   */
  onTool(name: string, handler: (input: Record<string, unknown>, context: WingsToolContext) => unknown): void;
  /**
   * Answers a Claude Code hook listed in `contributes.hooks` (Wings 0.1.54): `"SessionStart"` adds `context` to the
   * session, `"PreToolUse"` can add context and `deny` a Bash command with a reason Claude sees. Wings runs
   * `wings hook` for Claude Code once Connect has installed it. `"Journal"` (Wings 0.1.61) answers `wings journal`, which an agent's change journal runs at the end of each turn; its answer is ignored. An error, a slow answer (1.5 s) or no answer
   * counts as nothing, so Claude is never held up. `context` is capped at 10,000 characters. Register it in `main`.
   */
  onHook(event: "SessionStart" | "PreToolUse" | "Journal", handler: (input: WingsHookInput, context: WingsToolContext) => WingsHookAnswer | Promise<WingsHookAnswer>): void;
  /** Runs a command from `permissions.exec`, like `git branch --show-current`. Rejects with `Output is larger than 8 MB` (and kills the program) when stdout or stderr passes 8 MB. */
  exec(program: string, args?: string[], options?: WingsExecOptions): Promise<{ code: number | null; stdout: string; stderr: string }>;
  /**
   * Needs `permissions.statusline`. What Claude Code last told its status line, once `wings statusline` is your
   * `statusLine` command: usage limits and each session's context and cache, as Claude Code reports them.
   */
  statusline(): Promise<WingsStatusline>;
  /** Claude Code transcript entries of types in `permissions.transcript`, oldest first. */
  transcript(sessionId: string, types: string[], options?: { last?: number }): Promise<TranscriptEntry[]>;
  /**
   * Each transcript entry Claude Code writes while it runs in a pane, for the types in `permissions.transcript`.
   * Only entries written after the plugin started, or after the session started in the pane; `transcript` reads
   * earlier ones, and any written since, which `onTranscript` sends too. Entries over 256 KB are skipped.
   * To follow the sessions in panes, use `followSessions` from `wings-plugin-sdk/sessions`: it does both, once each.
   */
  onTranscript(listener: (event: WingsTranscriptEvent) => void): () => void;
  /**
   * Needs a `git` entry in `permissions.exec`, or it never fires. Wings sends the ids of the projects whose repo
   * moved: a commit, checkout, pull, fetch, push, amend or reset, from a pane or from anywhere else, once the writes
   * settle (at most one a second). Map an id to a repo with `projects` and `repoRoot`. It carries nothing else: no
   * paths, branch names or counts. Edits to files in the working tree don't count, so keep a slow poll for those,
   * and a pane in a repo that isn't a project gets nothing. Fires in the background too; `onGitChange` from
   * `wings-plugin-sdk/git` holds events until the page is in view. Needs Wings 0.1.50.
   */
  onGitChange(listener: (event: { projectIds: string[] }) => void): () => void;
  /**
   * Needs `"badges"` in `contributes.ui`. `null` removes it. Sending what this frame last sent for the pane does
   * nothing, `null` included, so it is safe to call on every poll. Wings drops a closed pane's badge, so send `null`
   * only to clear a pane that stays open. A call for a pane that has closed rejects with `no pane <id>`.
   */
  setBadge(paneId: string, badge: WingsBadge | null): Promise<void>;
  /**
   * Needs `"status"` in `contributes.ui`. Shows a slim strip under the pane's terminal, or removes it with `null`.
   * One status per plugin per pane; Wings removes it when the pane closes or the plugin stops. Rejects a status over
   * the limits (4 items, 16-character labels, 32-character values, 12 rows), a `percent` outside 0 to 100, or an
   * `open` that isn't one of the plugin's sidebars or views. Sending what this frame last sent for the pane does nothing.
   */
  setPaneStatus(paneId: string, status: WingsPaneStatus | null): Promise<void>;
  /**
   * Needs `"rail"` in `contributes.ui` and Wings 0.1.54. Floats a card over the pane's terminal, or removes it with
   * `null`. The user can drag it to a corner, minimise it and close it; a closed rail comes back when you send a
   * different one. One rail per plugin per pane; Wings removes it when the pane closes or the plugin stops. Rejects a
   * rail over the limits (40-character title, 30 items, 200-character text), an unknown `status`, or an `open` that
   * isn't one of the plugin's sidebars or views. Sending what this frame last sent for the pane does nothing.
   */
  setPaneRail(paneId: string, rail: WingsPaneRail | null): Promise<void>;
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
  openPane(options?: {
    command?: string;
    /** Values after `command`, one shell word each even with spaces, like a prompt for `claude`. Up to 4 KB together; none may start with `-` or hold a newline or other control character. Needs Wings 0.1.37. */
    args?: string[];
    cwd?: string;
    placement?: "tab" | "right" | "down";
  }): Promise<{ paneId: string }>;
  /** Needs `permissions.panes`. Shows an open pane and moves the keyboard to it. */
  focusPane(paneId: string): Promise<void>;
  /**
   * After `exec` rejected with "<program> is not installed or not on PATH": Wings asks you, installs it in a new tab
   * and restarts the plugin once it's there. Only `az`, `gh` and `glab`, and only ones `permissions.exec` names.
   * Resolves true once the install started (or it's already installed), false if you cancelled.
   */
  installTool(program: "az" | "gh" | "glab"): Promise<boolean>;
  /**
   * Needs `permissions.notify`. A desktop notification, up to 20 a minute. The title is cut at 64 characters, the body at 256.
   * It's also kept in Wings' Inbox, where `actions` (up to 3) are buttons: `open` shows one of your views that opens as a
   * tab, in `projectId`'s project or else the one on screen, and `url` opens a link your `openUrl` permission allows.
   * Needs Wings 0.1.45.
   */
  notify(notification: {
    title: string;
    body?: string;
    projectId?: string;
    actions?: ({ label: string; open: { view: string; args?: unknown } } | { label: string; url: string })[];
  }): Promise<void>;
  /** Needs `"diff"` in `contributes.ui`. Leave out `patch` to open it loading, then call `updateDiff`. */
  openDiff(diff: { title: string; subtitle?: string; /** The repo or folder the diff is of, so Send to agent offers the agent working there first. */ cwd?: string; patch?: string; comments?: WingsComment[]; focus?: WingsDiffFocus }): Promise<{ id: string }>;
  /** Updates a viewer opened with `openDiff`; ignored once it's closed. `{ comments }` alone keeps the patch, like after a reply. */
  updateDiff(id: string, update: { patch: string; comments?: WingsComment[]; focus?: WingsDiffFocus } | { comments: WingsComment[]; focus?: WingsDiffFocus } | { error: string }): Promise<void>;
  /**
   * Needs `"diff"` in `contributes.ui`. A reply typed under a comment with `canReply` in the diff viewer. Post it,
   * then add it with `updateDiff`. The reply box waits for the returned promise, and a thrown error shows under it
   * with the text kept. One handler: a second call replaces the first.
   */
  onReply(handler: (reply: WingsReply) => unknown): void;
  /**
   * POSTs `fields` (string or whole-number values, up to 64 KB) as JSON to a `permissions.post` path, as you:
   * `gh api` (against `host` for a GitHub Enterprise Server), `glab api` against `host`, or `az rest` for Azure
   * DevOps. Wings builds the command itself. A manifest entry may pin a field, like `{event=APPROVE}`.
   */
  post(
    program: "gh" | "glab" | "az",
    url: string,
    fields: Record<string, string | number>,
    options?: { host?: string },
  ): Promise<{ code: number | null; stdout: string; stderr: string }>;
  /**
   * An HTTP request to a URL under `permissions.fetch`, made by Wings. An entry may be `GET <prefix>`, or
   * `POST <url with * segments>` (optionally `{field=value}`) for one write with a JSON `body`. `bearer` names a secret that Wings sends
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
   * JSON. Keys are 1 to 128 letters, digits and `- _ . : /`. A value may be 1 MB and everything together 50 MB (1 MB in all
   * before Wings 0.1.50). Only this plugin sees it.
   */
  storage: {
    /** The value, or `null` when the key isn't set. */
    get(key: string): Promise<unknown>;
    /** Refused for a value over 1 MB, or if it would take the plugin's storage over 50 MB. */
    set(key: string, value: unknown): Promise<void>;
    delete(key: string): Promise<void>;
    /** Every key, sorted. */
    keys(): Promise<string[]>;
    /** `[key, value]` for every key that starts with `prefix` (all keys when empty), sorted by key. Needs Wings 0.1.50. */
    entries(prefix?: string): Promise<[string, unknown][]>;
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
   * already open with other args takes the new ones, drops its saved tab state and reloads; a view with `"multiple": true` opens a new tab,
   * unless a tab of it in that project already has the same `args`: that one is shown. Needs Wings 0.1.22.
   */
  openView(
    id: string,
    options?: {
      as?: "sidebar" | "modal" | "window" | "tab";
      args?: unknown;
      /** With `as: "tab"`: always a new tab, even when one of this view is open, like a middle-click. Needs Wings 0.1.39. */
      newTab?: boolean;
    },
  ): Promise<void>;
  /** Where this page shows. `null` in the plugin's main script. Needs Wings 0.1.22. */
  placement(): "sidebar" | "tab" | "modal" | "window" | "block" | null;
  /** What `openView` passed as `args` when it opened this tab, or `null`. Available at once. Needs Wings 0.1.22. */
  viewArgs<T = unknown>(): T | null;
  /**
   * The project this page is about, or `null` when none is open. A tab page's or block's project is the one it belongs to
   * and never changes; any other page gets the project on screen. Needs Wings 0.1.22.
   */
  project(): Promise<WingsProject | null>;
  /**
   * Calls the listener with the current project, then each time it changes: the project on screen switched (not for a
   * tab page), or its branch or name changed. Needs Wings 0.1.22.
   */
  onProject(listener: (project: WingsProject | null) => void): () => void;
  /** Every project in Wings, in sidebar order. Worktrees are projects too. Needs Wings 0.1.23. */
  projects(): Promise<WingsProject[]>;
  /**
   * Calls the listener with every project, then each time one is added, removed or renamed, or a branch changes.
   * Works in the main script and in pages. Needs Wings 0.1.23.
   */
  onProjects(listener: (projects: WingsProject[]) => void): () => void;
  /**
   * Shows `rows` under a project in the sidebar, in the section `sectionId` that the manifest declares in
   * `contributes.projectSections`, or removes the section with `null`. Up to 50 rows. Rejects for a project that isn't
   * there, a section the plugin didn't declare, or a row whose `open.view` isn't one of its views that opens as a tab.
   * Sections are kept in memory and go when the plugin is turned off, so fill them again on start. Sending what this
   * frame last sent for the project and section does nothing, so a poll can send every time; a project that leaves
   * the list is forgotten, so one added again is filled again. Needs Wings 0.1.23; skipping a repeat needs 0.1.50.
   */
  setProjectSection(projectId: string, sectionId: string, rows: WingsProjectSectionRow[] | null): Promise<void>;
  /** In a tab page: the state it last saved with `setTabState`, or `null`. Outside a tab it is `null`. Opening the tab with new `args` clears it. Needs Wings 0.1.22. */
  tabState<T = unknown>(): Promise<T | null>;
  /**
   * In a tab page: saves JSON state, up to 64 KB, on the tab. Wings keeps it across restarts and while the page is
   * unloaded. Rejects outside a tab or when the state is too big. Needs Wings 0.1.22.
   */
  setTabState(state: unknown): Promise<void>;
  /**
   * In a block on a Project page, `contributes.blocks`: the state it last saved with `setBlockState`, or `null`. Outside a
   * block it is `null`. Needs Wings 0.1.24.
   */
  blockState<T = unknown>(): Promise<T | null>;
  /**
   * In a block: saves JSON state, up to 16 KB, on the block, like which board or filter it shows. Wings keeps it with the
   * Project page across restarts and while the block is unloaded. Rejects outside a block or when the state is too big.
   * Needs Wings 0.1.24.
   */
  setBlockState(state: unknown): Promise<void>;
  /**
   * In a block: asks for a height in CSS pixels. Wings rounds it and clamps it to the block's `minHeight` (default 160)
   * up to 1200, and applies it without reloading the page. Call it when the content's height changes: asking for the
   * height already asked for does nothing, and calls in the same tick send only the last. Throws outside a block. Needs Wings 0.1.24.
   */
  setHeight(px: number): void;
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
