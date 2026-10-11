import { Channel } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { ImageAddon } from "@xterm/addon-image";
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
  | { kind: "newTab" | "splitRight" | "splitDown" | "toggleSidebar" | "fontUp" | "fontDown" | "fontReset" | "reload" | "nextAgent" | "previousAgent" }
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
  const fromCode: Record<string, string> = { KeyT: "t", KeyD: "d", KeyB: "b", KeyJ: "j", KeyR: "r", Backslash: "\\", NumpadAdd: "+", NumpadSubtract: "-", Numpad0: "0" };
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
    case "j":
      // The next agent that needs you; with ⇧ the one before.
      return { kind: extra ? "previousAgent" : "nextAgent" };
    case "r":
      // Reloads only the UI; the terminals keep running and come back. For trying UI changes in dev.
      return import.meta.env.DEV && !extra ? { kind: "reload" } : null;
  }
  if (key && /^[1-9]$/.test(key)) return { kind: "tab", index: Number(key) - 1 };
  return null;
}

export const shortcutLabel = isMac
  ? { newTab: "⌘T", splitRight: "⌘D", splitDown: "⇧⌘D", toggleSidebar: "⌘B", closePane: "⌘W", nextAgent: "⌘J" }
  : {
      newTab: "Ctrl+Shift+T",
      splitRight: "Ctrl+Shift+D",
      splitDown: "Ctrl+Shift+Alt+D",
      toggleSidebar: "Ctrl+Shift+B",
      closePane: "Ctrl+Shift+W",
      nextAgent: "Ctrl+Shift+J",
    };

export const DEFAULT_FONT_SIZE = 13;
let fontSize = DEFAULT_FONT_SIZE;

/** Applies a font size to every terminal, open or future. */
export function setTerminalFontSize(size: number) {
  fontSize = size;
  for (const session of terminals.values()) session.setFontSize(size);
}

const FONT_STACK = "ui-monospace, 'SF Mono', Menlo, Monaco, Consolas, 'Liberation Mono', monospace";

export type CursorStyle = "block" | "underline" | "bar";
export type CursorInactiveStyle = "outline" | "block" | "bar" | "underline" | "none";
/** How the terminal looks and selects, as in a native terminal's profile settings. */
export type TerminalLook = {
  /** Tried before the default mono stack. Empty means the default alone. */
  fontFamily: string;
  lineHeight: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  cursorInactiveStyle: CursorInactiveStyle;
  /** Lines of history per pane. Each full line costs memory, about 49 MB per pane at 10,000 on a Retina display. */
  scrollback: number;
  copyOnSelect: boolean;
  minimumContrastRatio: number;
  /** Draws images that programs print (SIXEL, iTerm2 inline images). */
  images: boolean;
};
export const DEFAULT_TERMINAL_LOOK: TerminalLook = {
  fontFamily: "",
  lineHeight: 1.2,
  cursorStyle: "block",
  cursorBlink: true,
  cursorInactiveStyle: "outline",
  scrollback: 10_000,
  copyOnSelect: false,
  minimumContrastRatio: 1,
  images: true,
};
/**
 * Memory caps per terminal for inline images. The addon's own defaults (128 MB stored, 4096 x 4096 pixels per image)
 * would let one pane cost more than its whole scrollback (about 49 MB). 24 MB holds about six 1024 x 1024 images and
 * the oldest are dropped first; a single image is capped at 2048 x 2048 (16 MB decoded), and the raw sequence at 8 MB.
 */
const IMAGE_OPTIONS = {
  storageLimit: 24,
  pixelLimit: 2048 * 2048,
  sixelSizeLimit: 8_000_000,
  iipSizeLimit: 8_000_000,
};
let look = DEFAULT_TERMINAL_LOOK;
let lookTimer: ReturnType<typeof setTimeout> | undefined;

const fontStack = (l: TerminalLook) => (l.fontFamily.trim() ? `${l.fontFamily.trim()}, ${FONT_STACK}` : FONT_STACK);

/**
 * Applies the look to every terminal, open or future. A font family or line height changes the cell size, and each
 * terminal then remeasures, refits and resizes its PTY, so those wait until typing or clicking has settled and reach
 * all terminals in one pass. The rest apply at once.
 */
export function setTerminalLook(next: TerminalLook) {
  const prev = look;
  look = next;
  const cellChanged = prev.fontFamily !== next.fontFamily || prev.lineHeight !== next.lineHeight;
  for (const session of terminals.values()) session.applyLook(next, false);
  if (!cellChanged) return;
  clearTimeout(lookTimer);
  lookTimer = setTimeout(() => {
    for (const session of terminals.values()) session.applyLook(look, true);
  }, 250);
}

export type OptionAsMeta = "off" | "left" | "right" | "both";
/** How keys reach the shell, as in a native terminal's keyboard settings. */
export type TerminalKeys = { optionAsMeta: OptionAsMeta; shiftReturnSendsMetaReturn: boolean };
// Left Option as Meta keeps the right one for characters like ~ on non-US layouts.
export const DEFAULT_TERMINAL_KEYS: TerminalKeys = { optionAsMeta: "left", shiftReturnSendsMetaReturn: true };
let keys = DEFAULT_TERMINAL_KEYS;

export function setTerminalKeys(next: TerminalKeys) {
  keys = next;
}

/** The Option keys held down. xterm.js treats Option as Meta or not, never one side only, so it's switched per key. */
const heldOption = new Set<string>();

function optionIsMeta(e: KeyboardEvent) {
  if (e.code === "AltLeft" || e.code === "AltRight") {
    if (e.type === "keyup") heldOption.delete(e.code);
    else heldOption.add(e.code);
  }
  if (!e.altKey) heldOption.clear();
  const side = keys.optionAsMeta;
  return side === "both" || (side === "left" && heldOption.has("AltLeft")) || (side === "right" && heldOption.has("AltRight"));
}

/** Live terminals by pane key. Kept outside React state: they own PTYs and DOM nodes. */
export const terminals = new Map<string, TerminalSession>();

/** One xterm.js instance bound to one PTY in the Rust core. Lives outside React so panes survive re-renders. */
export class TerminalSession {
  readonly el = document.createElement("div");
  readonly term = new Terminal({
    fontFamily: fontStack(look),
    fontSize,
    lineHeight: look.lineHeight,
    scrollback: look.scrollback,
    cursorBlink: look.cursorBlink,
    cursorStyle: look.cursorStyle,
    cursorInactiveStyle: look.cursorInactiveStyle,
    minimumContrastRatio: look.minimumContrastRatio,
    macOptionIsMeta: false,
    theme,
  });
  paneId: string | null = null;
  private fit = new FitAddon();
  private webgl: WebglAddon | null = null;
  private image: ImageAddon | null = null;
  private visible = false;
  private started = false;
  private initialInput: string | null = null;
  // A sidebar slide or a drag changes the size every frame. Each refit reflows the scrollback and resizes the
  // PTY, which makes the shell or Claude redraw, so refit once the size has held still.
  private settle: ReturnType<typeof setTimeout> | undefined;
  private resizeObserver = new ResizeObserver(() => {
    clearTimeout(this.settle);
    this.settle = setTimeout(() => this.refit(), 80);
  });
  private resolveStarted!: (paneId: string | null) => void;
  /** The Rust pane id once the shell has started, or null if it couldn't start. */
  readonly whenStarted = new Promise<string | null>((resolve) => (this.resolveStarted = resolve));

  constructor(
    readonly spaceId: string,
    private handlers: { onFocus: () => void; onStarted: (paneId: string) => void },
    /** A folder inside the project to start in, instead of its root. */
    private cwd: string | null = null,
    /** A Rust pane still running from before a webview reload, to show again instead of starting a shell. */
    private attachTo: string | null = null,
  ) {
    this.el.className = "h-full w-full";
    this.term.loadAddon(this.fit);
    // ⌘/Ctrl-click opens a link, so a plain click (which Claude Code's mouse mode uses) never opens one by accident.
    this.term.loadAddon(
      new WebLinksAddon((event, uri) => {
        if (event.metaKey || event.ctrlKey) void openUrl(uri);
      }),
    );
    this.term.attachCustomKeyEventHandler((e) => {
      const meta = optionIsMeta(e);
      if (this.term.options.macOptionIsMeta !== meta) this.term.options.macOptionIsMeta = meta;
      // Meta Return is a new line in Claude Code, where a plain Return would send the prompt.
      if (keys.shiftReturnSendsMetaReturn && e.key === "Enter" && e.shiftKey && !e.altKey && !e.metaKey && !e.ctrlKey) {
        if (e.type === "keydown") this.term.input("\x1b\r");
        return false;
      }
      return shortcutFor(e) === null;
    });
    this.el.addEventListener("focusin", handlers.onFocus);
    // Copy on select writes when the button is released, not on every selection change while dragging.
    // The release can land outside the pane, so listen on the document once a press starts inside it.
    this.el.addEventListener("mousedown", (e) => {
      if (e.button !== 0) return;
      document.addEventListener("mouseup", () => this.copySelection(), { once: true });
    });
  }

  private copySelection() {
    if (!look.copyOnSelect || !this.term.hasSelection()) return;
    void navigator.clipboard.writeText(this.term.getSelection()).catch(() => {});
  }

  /**
   * Sets the look on this terminal. Works before the terminal opens and while it's hidden. The cell size only
   * changes the PTY while visible: a hidden terminal refits when it's shown (`setVisible`).
   * `cell` is false for the immediate pass, true for the debounced one that carries the font and line height.
   */
  applyLook(next: TerminalLook, cell: boolean) {
    const o = this.term.options;
    if (o.cursorStyle !== next.cursorStyle) o.cursorStyle = next.cursorStyle;
    if (o.cursorBlink !== next.cursorBlink) o.cursorBlink = next.cursorBlink;
    if (o.cursorInactiveStyle !== next.cursorInactiveStyle) o.cursorInactiveStyle = next.cursorInactiveStyle;
    if (o.scrollback !== next.scrollback) o.scrollback = next.scrollback;
    if (o.minimumContrastRatio !== next.minimumContrastRatio) o.minimumContrastRatio = next.minimumContrastRatio;
    if (this.started) this.setImages(next.images);
    if (!cell) return;
    const family = fontStack(next);
    if (o.fontFamily === family && o.lineHeight === next.lineHeight) return;
    o.fontFamily = family;
    o.lineHeight = next.lineHeight;
    this.refit();
  }

  /** Puts the terminal in `host`. xterm opens and the PTY spawns the first time it's shown (see `setVisible`). */
  attach(host: HTMLElement, initialInput: string | null) {
    this.initialInput = initialInput;
    if (this.el.parentElement === host) return;
    host.appendChild(this.el);
    if (this.started) {
      // A moved canvas keeps its pixels from before the move; repaint at the new size.
      this.refit();
      this.term.refresh(0, this.term.rows - 1);
    }
  }

  private async start(initialInput: string | null) {
    this.term.open(this.el);
    this.visible = true;
    this.setImages(look.images);
    this.enableWebgl();
    this.fit.fit();
    this.resizeObserver.observe(this.el);

    // A failed attach ends its channel, so the shell started instead gets a new one.
    const output = () => new Channel<ArrayBuffer>((buf) => this.term.write(new Uint8Array(buf)));
    const attachTo = this.attachTo;
    const attached = attachTo !== null && (await api.paneAttach(attachTo, output()).then(() => true, () => false));
    // The pane exited during the reload: start over, as after a restart (a Claude pane resumes its session).
    this.paneId = attached
      ? attachTo
      : await api.paneCreate(this.spaceId, this.term.cols, this.term.rows, initialInput, output(), this.cwd).catch((e) => {
          this.resolveStarted(null);
          throw e;
        });
    const id = this.paneId;
    if (attached) this.repaint(id);
    this.term.onData((data) => void api.paneWrite(id, data));
    this.term.onResize(({ cols, rows }) => void api.paneResize(id, cols, rows));
    this.handlers.onStarted(id);
    this.resolveStarted(id);
  }

  /**
   * Makes the program in a reattached pane draw its screen again at this terminal's size. Resizing to the size the
   * PTY already has signals nothing, and Node (Claude Code) redraws only when the size it reads has changed, so step
   * a row off and back, far enough apart that it reads both.
   */
  private repaint(id: string) {
    const { cols, rows } = this.term;
    void api.paneResize(id, cols, rows > 1 ? rows - 1 : rows + 1);
    setTimeout(() => void api.paneResize(id, this.term.cols, this.term.rows), 100);
  }

  /**
   * A restored tab's shells start the first time it's shown, so tabs nobody opens cost no PTY or terminal.
   * `keepRenderer` lets a hidden pane keep its WebGL renderer, for the tab shown just before.
   */
  setVisible(visible: boolean, keepRenderer = false) {
    if (visible && !this.started) {
      this.started = true;
      void this.start(this.initialInput);
      return;
    }
    if (visible !== this.visible) {
      this.visible = visible;
      if (visible) {
        this.enableWebgl();
        this.refit();
      }
    }
    // WebKit allows 16 WebGL contexts per page and silently drops the oldest, so hidden panes give theirs up.
    if (!visible && !keepRenderer) {
      this.disableWebgl();
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
    clearTimeout(this.settle);
    this.resizeObserver.disconnect();
    this.disableWebgl();
    this.setImages(false);
    if (this.paneId) void api.paneClose(this.paneId);
    this.term.dispose();
    this.el.remove();
  }

  private refit() {
    if (this.visible && this.el.isConnected && this.el.clientWidth > 0) this.fit.fit();
  }

  /** Loaded when the terminal starts, not at construction, so unopened tabs hold no image storage. */
  private setImages(on: boolean) {
    if (on === (this.image !== null)) return;
    if (!on) {
      this.image?.dispose();
      this.image = null;
      return;
    }
    this.image = new ImageAddon(IMAGE_OPTIONS);
    this.term.loadAddon(this.image);
  }

  private disableWebgl() {
    if (!this.webgl) return;
    const canvases = [...this.el.querySelectorAll("canvas")];
    this.webgl.dispose();
    this.webgl = null;
    // The addon only removes its canvas, so the context and its GPU memory stay until the canvas is garbage
    // collected. Lose it now. The addon's 2D link layer canvas returns null here.
    for (const canvas of canvases) {
      if (!canvas.isConnected) canvas.getContext("webgl2")?.getExtension("WEBGL_lose_context")?.loseContext();
    }
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
