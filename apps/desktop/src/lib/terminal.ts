import { Channel } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { api } from "@/lib/api";

const theme = {
  // Opaque and equal to the pane card (--surface): xterm's WebGL renderer draws dim text on black boxes
  // when the background is transparent, and transparency also slows it down.
  background: "#17151b",
  foreground: "#e9e6e1",
  cursor: "#f0ede8",
  cursorAccent: "#16141a",
  selectionBackground: "#ffffff33",
  scrollbarSliderBackground: "#ffffff1a",
  scrollbarSliderHoverBackground: "#ffffff33",
  scrollbarSliderActiveBackground: "#ffffff4d",
  black: "#4a4650",
  red: "#f2786a",
  green: "#8fd19e",
  yellow: "#e9c46a",
  blue: "#7aa7f2",
  magenta: "#d59cf0",
  cyan: "#6fd0d0",
  white: "#d8d4ce",
  brightBlack: "#6d6872",
  brightRed: "#ff9285",
  brightGreen: "#a7e3b3",
  brightYellow: "#f3d68b",
  brightBlue: "#9cc0ff",
  brightMagenta: "#e4b8f7",
  brightCyan: "#93e2e2",
  brightWhite: "#f4f1ec",
};

export const isMac = /Mac/.test(navigator.userAgent);

export type Action =
  | { kind: "newTab" | "splitRight" | "splitDown" | "toggleSidebar" | "fontUp" | "fontDown" | "fontReset" }
  | { kind: "tab"; index: number }
  | { kind: "focus"; dir: "left" | "right" | "up" | "down" };

/**
 * App shortcuts: ⌘ on macOS, Ctrl+Shift elsewhere (plain Ctrl belongs to the shell).
 * Matches the character typed, like macOS menus do, so it works on any keyboard layout
 * (on Croatian-PC, − and + aren't where they are on a US keyboard). Physical keys are only
 * the fallback for layouts that don't type Latin characters.
 */
export function shortcutFor(e: KeyboardEvent): Action | null {
  const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && e.shiftKey && !e.metaKey;
  if (!mod) return null;
  if (e.altKey && e.code.startsWith("Arrow")) {
    return { kind: "focus", dir: e.code.slice(5).toLowerCase() as "left" | "right" | "up" | "down" };
  }
  const typed = e.key.length === 1 && /[\x21-\x7e]/.test(e.key) ? e.key.toLowerCase() : null;
  const fromCode: Record<string, string> = { KeyT: "t", KeyD: "d", KeyB: "b", Backslash: "\\", NumpadAdd: "+", NumpadSubtract: "-", Numpad0: "0" };
  const key = typed ?? fromCode[e.code] ?? (/^Digit\d$/.test(e.code) ? e.code.slice(5) : null);
  const extra = isMac ? e.shiftKey : e.altKey;
  switch (key) {
    case "t":
      return { kind: "newTab" };
    case "d":
      return { kind: extra ? "splitDown" : "splitRight" };
    case "b":
    case "\\":
      return { kind: "toggleSidebar" };
    case "+":
    case "=":
      return { kind: "fontUp" };
    case "-":
    case "_":
      return { kind: "fontDown" };
    case "0":
      return { kind: "fontReset" };
  }
  if (key && /^[1-9]$/.test(key)) return { kind: "tab", index: Number(key) - 1 };
  return null;
}

export const shortcutLabel = isMac
  ? { newTab: "⌘T", splitRight: "⌘D", splitDown: "⇧⌘D", toggleSidebar: "⌘B", closePane: "⌘W" }
  : {
      newTab: "Ctrl+Shift+T",
      splitRight: "Ctrl+Shift+D",
      splitDown: "Ctrl+Shift+Alt+D",
      toggleSidebar: "Ctrl+Shift+B",
      closePane: "Ctrl+Shift+W",
    };

export const DEFAULT_FONT_SIZE = 13;
let fontSize = DEFAULT_FONT_SIZE;

/** Applies a font size to every terminal, open or future. */
export function setTerminalFontSize(size: number) {
  fontSize = size;
  for (const session of terminals.values()) session.setFontSize(size);
}

/** Live terminals by pane key. Kept outside React state: they own PTYs and DOM nodes. */
export const terminals = new Map<string, TerminalSession>();

/** One xterm.js instance bound to one PTY in the Rust core. Lives outside React so panes survive re-renders. */
export class TerminalSession {
  readonly el = document.createElement("div");
  readonly term = new Terminal({
    fontFamily: "ui-monospace, 'SF Mono', Menlo, Monaco, Consolas, 'Liberation Mono', monospace",
    fontSize,
    lineHeight: 1.2,
    scrollback: 10_000,
    cursorBlink: true,
    macOptionIsMeta: true,
    theme,
  });
  paneId: string | null = null;
  private fit = new FitAddon();
  private webgl: WebglAddon | null = null;
  private visible = false;
  private started = false;
  private resizeObserver = new ResizeObserver(() => this.refit());

  constructor(
    readonly spaceId: string,
    private handlers: { onFocus: () => void; onStarted: (paneId: string) => void },
  ) {
    this.el.className = "h-full w-full";
    this.term.loadAddon(this.fit);
    // ⌘/Ctrl-click opens a link, so a plain click (which Claude Code's mouse mode uses) never opens one by accident.
    this.term.loadAddon(
      new WebLinksAddon((event, uri) => {
        if (event.metaKey || event.ctrlKey) void openUrl(uri);
      }),
    );
    this.term.attachCustomKeyEventHandler((e) => shortcutFor(e) === null);
    this.el.addEventListener("focusin", handlers.onFocus);
  }

  /** Puts the terminal in `host`. The first call also opens xterm and spawns the PTY; later calls just move it. */
  attach(host: HTMLElement, initialInput: string | null) {
    if (this.started) {
      if (this.el.parentElement !== host) {
        host.appendChild(this.el);
        // A moved canvas keeps its pixels from before the move; repaint at the new size.
        this.refit();
        this.term.refresh(0, this.term.rows - 1);
      }
      return;
    }
    host.appendChild(this.el);
    this.started = true;
    void this.start(initialInput);
  }

  private async start(initialInput: string | null) {
    this.term.open(this.el);
    this.visible = true;
    this.enableWebgl();
    this.fit.fit();
    this.resizeObserver.observe(this.el);

    const output = new Channel<ArrayBuffer>((buf) => this.term.write(new Uint8Array(buf)));
    this.paneId = await api.paneCreate(this.spaceId, this.term.cols, this.term.rows, initialInput, output);
    const id = this.paneId;
    this.term.onData((data) => void api.paneWrite(id, data));
    this.term.onResize(({ cols, rows }) => void api.paneResize(id, cols, rows));
    this.handlers.onStarted(id);
  }

  setVisible(visible: boolean) {
    if (visible === this.visible) return;
    this.visible = visible;
    if (visible) {
      this.enableWebgl();
      this.refit();
    } else {
      // WebKit allows 16 WebGL contexts per page and silently drops the oldest, so only visible panes hold one.
      this.webgl?.dispose();
      this.webgl = null;
    }
  }

  focus() {
    this.term.focus();
  }

  setFontSize(size: number) {
    if (this.term.options.fontSize === size) return;
    this.term.options.fontSize = size;
    this.refit();
  }

  dispose() {
    this.resizeObserver.disconnect();
    if (this.paneId) void api.paneClose(this.paneId);
    this.term.dispose();
    this.el.remove();
  }

  private refit() {
    if (this.visible && this.el.isConnected && this.el.clientWidth > 0) this.fit.fit();
  }

  private enableWebgl() {
    if (this.webgl || !this.el.isConnected) return;
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        addon.dispose();
        this.webgl = null;
      });
      this.term.loadAddon(addon);
      this.webgl = addon;
    } catch {
      // No WebGL in this webview: xterm keeps its DOM renderer.
      this.webgl = null;
    }
  }
}
