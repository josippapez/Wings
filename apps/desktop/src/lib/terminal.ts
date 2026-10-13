import { Channel } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ClipboardAddon, type ClipboardSelectionType, type IClipboardProvider } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon, type ISearchOptions } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { ImageAddon } from "@xterm/addon-image";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { api } from "@/lib/api";
import { commandForKey, shortcutOf } from "@/lib/commands";
import { editKey } from "@/lib/edit-keys";
import { canUnloadHistory, HISTORY_SEAM, TERMINAL_UNLOAD_MS } from "@/lib/idle-unload";

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

/** The command a key press is (see `commandForKey`), or null if it isn't an app shortcut. */
export const shortcutFor = (e: KeyboardEvent) => commandForKey(e, isMac, import.meta.env.DEV);

/** Shortcut labels for the buttons that show one. */
export const shortcutLabel = {
  newTab: shortcutOf("new-tab", isMac),
  splitRight: shortcutOf("split-right", isMac),
  splitDown: shortcutOf("split-down", isMac),
  toggleSidebar: shortcutOf("toggle-sidebar", isMac),
  closePane: shortcutOf("close-pane", isMac),
  nextAgent: shortcutOf("next-agent", isMac),
  find: shortcutOf("find", isMac),
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
  /** Lets programs put text on the clipboard with OSC 52. Reading the clipboard that way is never allowed. */
  clipboardWrite: boolean;
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
  clipboardWrite: true,
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
/**
 * What OSC 52 may do. A write goes to the system clipboard while the setting is on. A read always answers with an
 * empty clipboard: allowing it would let any program in a pane, including one on a remote machine, read what you
 * copied.
 */
const clipboardProvider: IClipboardProvider = {
  readText: () => "",
  writeText: async (_selection: ClipboardSelectionType, text: string) => {
    if (look.clipboardWrite) await navigator.clipboard.writeText(text).catch(() => {});
  },
};

/** The match colours must be #RRGGBB, which the addon needs for its decorations. */
const SEARCH_DECORATIONS = {
  matchBackground: "#5b4b1f",
  matchBorder: "#e9c46a",
  matchOverviewRuler: "#e9c46a",
  activeMatchBackground: "#e9a23b",
  activeMatchBorder: "#ffd98a",
  activeMatchColorOverviewRuler: "#ffd98a",
};
export type FindOptions = { caseSensitive: boolean; regex: boolean };
export type FindResults = { index: number; count: number };

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

/**
 * How long the tab shown just before keeps its terminals' WebGL renderers once hidden. Making one again when it's
 * shown costs about 50 ms (three frames) in WKWebView, measured.
 */
const RENDERER_KEEP_MS = 30_000;

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
    // The Unicode and search addons use xterm's proposed API (unicode versions, decorations).
    allowProposedApi: true,
    theme,
  });
  paneId: string | null = null;
  private fit = new FitAddon();
  private webgl: WebglAddon | null = null;
  private image: ImageAddon | null = null;
  private search: SearchAddon | null = null;
  private findListeners = new Set<() => void>();
  private findOpenState = false;
  private findTick = 0;
  private findResults: FindResults = { index: -1, count: 0 };
  private visible = false;
  private started = false;
  private disposed = false;
  // A pane hidden for TERMINAL_UNLOAD_MS stores its scrollback as serialized bytes (about 16x smaller than the
  // buffer, measured) and writes it back when shown. Output that arrives during the rewrite waits in `held`.
  private unloadTimer: ReturnType<typeof setTimeout> | undefined;
  private releaseTimer: ReturnType<typeof setTimeout> | undefined;
  private history: Uint8Array | null = null;
  private serializer: SerializeAddon | null = null;
  private restoring = false;
  private held: Uint8Array[] = [];
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
      const edit = isMac ? editKey(e) : null;
      if (edit) {
        if (e.type === "keydown") this.term.input(edit);
        return false;
      }
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
    // xterm's scrollbar sliders carry translate3d(0, 0, 0) only as a layer hint (they move by top and left). In WebKit
    // that layer gets each pane card, and the hidden tabs stacked over it, composited with backing stores the size of
    // the pane: about 100 MB of graphics memory for 5 panes on a Retina screen (measured). The slider repaints instead.
    for (const slider of this.el.querySelectorAll<HTMLElement>(".xterm-scrollable-element .slider")) slider.style.transform = "";
    this.visible = true;
    // Before the first write: the width rules decide how every line of output is laid out.
    const unicode = new Unicode11Addon();
    const search = new SearchAddon();
    this.term.loadAddon(unicode);
    this.term.unicode.activeVersion = "11";
    this.term.loadAddon(search);
    this.term.loadAddon(new ClipboardAddon(undefined, clipboardProvider));
    this.search = search;
    search.onDidChangeResults(({ resultIndex, resultCount }) => {
      this.findResults = { index: resultIndex, count: resultCount };
      this.notifyFind();
    });
    this.setImages(look.images);
    this.enableWebgl();
    this.fit.fit();
    this.resizeObserver.observe(this.el);

    // A failed attach ends its channel, so the shell started instead gets a new one.
    const output = () => new Channel<ArrayBuffer>((buf) => this.receive(new Uint8Array(buf)));
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
   * `keepRenderer` lets a hidden pane keep its WebGL renderer for `RENDERER_KEEP_MS`, for the tab shown just before.
   */
  setVisible(visible: boolean, keepRenderer = false) {
    if (visible && !this.started) {
      this.started = true;
      void this.start(this.initialInput);
      return;
    }
    if (visible !== this.visible) {
      this.visible = visible;
      clearTimeout(this.unloadTimer);
      if (visible) {
        this.enableWebgl();
        // The scrollback goes back at the columns it was stored at, then the pane refits and reflows it.
        if (this.history === null) this.refit();
        else void this.restoreHistory().then(() => this.refit());
      } else {
        this.unloadTimer = setTimeout(() => this.unload(), TERMINAL_UNLOAD_MS);
      }
    }
    clearTimeout(this.releaseTimer);
    if (visible) return;
    // WebKit allows 16 WebGL contexts per page and silently drops the oldest, so hidden panes give theirs up. The tab
    // shown just before keeps them for a while, so switching straight back stays quick, then gives them up too: a kept
    // renderer and the layers it composites cost about 100 MB of graphics memory on a Retina screen (measured).
    if (keepRenderer) this.releaseTimer = setTimeout(() => this.disableWebgl(), RENDERER_KEEP_MS);
    else this.disableWebgl();
  }

  private receive(data: Uint8Array) {
    if (this.restoring) this.held.push(data);
    else this.term.write(data);
  }

  /** Hidden long enough: gives up the renderer, and keeps the scrollback as text instead of buffer lines. */
  private unload() {
    this.disableWebgl();
    const normal = this.term.buffer.normal;
    const state = {
      started: this.started,
      visible: this.visible,
      restoring: this.restoring,
      alternate: this.term.buffer.active.type === "alternate",
      historyLines: normal.baseY,
      unloaded: this.history !== null,
    };
    if (!canUnloadHistory(state)) return;
    this.serializer ??= new SerializeAddon();
    this.term.loadAddon(this.serializer);
    // As bytes: the addon builds the string with `+=`, and a string kept like that stays a tree of millions of
    // pieces (290 MB for 15 panes, measured), where a copy as bytes is flat.
    this.history = new TextEncoder().encode(this.serializer.serialize({ range: { start: 0, end: normal.baseY - 1 }, excludeModes: true, excludeAltBuffer: true }));
    // Drops the buffer's lines; setting it back keeps the option, and new lines only cost memory as they arrive.
    const scrollback = this.term.options.scrollback;
    this.term.options.scrollback = 0;
    this.term.options.scrollback = scrollback;
  }

  /** Puts the stored scrollback back in front of what arrived while it was away. */
  private async restoreHistory() {
    const history = this.history;
    if (history === null || !this.serializer) return;
    this.history = null;
    this.restoring = true;
    try {
      // Everything already written has to be in the buffer before it's serialized.
      await new Promise<void>((done) => this.term.write("", done));
      if (this.disposed) return;
      const tail = this.serializer.serialize({ excludeAltBuffer: false });
      this.term.reset();
      this.term.write(history);
      this.term.write(HISTORY_SEAM + tail);
    } finally {
      this.restoring = false;
      const held = this.held.splice(0);
      if (!this.disposed) for (const data of held) this.term.write(data);
    }
  }

  focus() {
    this.term.focus();
  }

  // The find bar's state lives here, with the terminal, so a shortcut can open it without going through React state.
  subscribeFind = (listener: () => void) => {
    this.findListeners.add(listener);
    return () => void this.findListeners.delete(listener);
  };

  private notifyFind() {
    for (const listener of this.findListeners) listener();
  }

  readonly getFindOpen = () => this.findOpenState;
  /** Counts ⌘F presses, so the bar refocuses its input when it's already open. */
  readonly getFindTick = () => this.findTick;
  readonly getFindResults = () => this.findResults;

  openFind() {
    if (!this.search) return;
    this.findOpenState = true;
    this.findTick++;
    this.notifyFind();
  }

  closeFind() {
    if (!this.findOpenState) return;
    this.findOpenState = false;
    this.search?.clearDecorations();
    this.findResults = { index: -1, count: 0 };
    this.notifyFind();
    this.term.focus();
  }

  /** Moves to the next or previous match. An empty term, or a regex that doesn't parse, finds nothing. */
  find(term: string, options: FindOptions, direction: "next" | "previous", incremental = false) {
    const search = this.search;
    if (!search) return false;
    if (!term) {
      search.clearDecorations();
      this.findResults = { index: -1, count: 0 };
      this.notifyFind();
      return false;
    }
    const opts: ISearchOptions = { ...options, incremental, decorations: SEARCH_DECORATIONS };
    try {
      const found = direction === "next" ? search.findNext(term, opts) : search.findPrevious(term, opts);
      if (!found) {
        this.findResults = { index: -1, count: 0 };
        this.notifyFind();
      }
      return found;
    } catch {
      return false;
    }
  }

  setFontSize(size: number) {
    if (this.term.options.fontSize === size) return;
    this.term.options.fontSize = size;
    this.refit();
  }

  dispose() {
    this.disposed = true;
    clearTimeout(this.unloadTimer);
    clearTimeout(this.settle);
    this.resizeObserver.disconnect();
    this.disableWebgl();
    this.setImages(false);
    this.search?.dispose();
    this.search = null;
    this.findListeners.clear();
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
    // @xterm/addon-webgl 0.19 never disposes its cursor blink timer (fixed upstream in xterm.js 7a6e724, not released
    // yet). A renderer disposed while its terminal has focus keeps that timer running, and the timer holds the
    // renderer, its canvases and their GPU memory for good: 59 MB for a 1500 x 1000 pane on a Retina screen
    // (measured). A blur stops the timer first. Every caller is hiding or closing the pane, so the focus goes anyway.
    if (this.term.textarea === document.activeElement) this.term.blur();
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
