// Types for the Wings plugin SDK, API 1. `window.wings` exists in every plugin; this file only helps your editor.
// Add `// @ts-check` and `/// <reference path="./wings.d.ts" />` at the top of main.js to use it.

type Tone = "neutral" | "info" | "success" | "warning" | "danger" | "merged";

interface WingsPane {
  paneId: string;
  /** Follows `cd` in the pane. */
  cwd: string;
  /** What runs in the pane right now, like `zsh` or `claude`. */
  command: string;
  project: string;
  /** Set while Claude Code runs in the pane. */
  session: { sessionId: string; name: string | null; state: string } | null;
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
}

interface WingsExecOptions {
  cwd?: string;
  /** 30 s by default, 5 min at most. */
  timeoutMs?: number;
  /** Gets each stdout and stderr line while the program runs. */
  onOutput?: (line: string) => void;
}

interface Wings {
  /** Every terminal pane, on every change. */
  onPanes(listener: (panes: WingsPane[]) => void): void;
  /** A badge action was clicked. Return a promise: the button spins until it settles, and a thrown error shows in the card. */
  onAction(listener: (action: { paneId: string; actionId: string }) => void | Promise<void>): void;
  /**
   * Handles Claude's calls to a tool in `contributes.mcpTools`. Return a string or a JSON value; a thrown error
   * is sent back as a failed call. `paneId` is the Wings pane Claude runs in, or `null`. Register it in `main`.
   */
  onTool(name: string, handler: (input: Record<string, unknown>, context: { paneId: string | null }) => unknown): void;
  /** Runs a command from `permissions.exec`, like `git branch --show-current`. */
  exec(program: string, args?: string[], options?: WingsExecOptions): Promise<{ code: number | null; stdout: string; stderr: string }>;
  /** Claude Code transcript entries of types in `permissions.transcript`, oldest first. */
  transcript(sessionId: string, types: string[]): Promise<Record<string, unknown>[]>;
  /** Needs `"badges"` in `contributes.ui`. `null` removes it. */
  setBadge(paneId: string, badge: WingsBadge | null): Promise<void>;
  /** Opens an https URL that starts with a prefix in `permissions.openUrl`. */
  openUrl(url: string): Promise<void>;
  /** Needs `"diff"` in `contributes.ui`. Leave out `patch` to open it loading, then call `updateDiff`. */
  openDiff(diff: { title: string; subtitle?: string; patch?: string; comments?: WingsComment[] }): Promise<{ id: string }>;
  updateDiff(id: string, update: { patch: string; comments?: WingsComment[] } | { error: string }): Promise<void>;
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
  /** Short text next to a sidebar's title bar button, like a running timer. `null` clears it. */
  setSidebarLabel(sidebarId: string, label: string | null, options?: { rows?: { label: string; value: string }[] }): Promise<void>;
  /** Sends a JSON value, up to 64 KB, to the plugin's other pages: its main script and open sidebars. */
  broadcast(message: unknown): Promise<void>;
  /** Gets what the plugin's other pages send with `broadcast`. */
  onBroadcast(listener: (message: unknown) => void): void;
}

declare const wings: Wings;
