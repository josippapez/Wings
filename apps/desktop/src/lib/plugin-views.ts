/**
 * A plugin view opened as a window. The window loads the Wings UI, which sees its `view:` label and shows only
 * the plugin's sandboxed frame. It has no app commands (see `capabilities/plugin-view.json`), so it hands each
 * message from the frame to the main window's plugin host as an event, and the host answers the same way. Every
 * call is then checked where the sidebar's calls are, in `PluginHost.handle` and in Rust.
 */

export const VIEW_EVENTS = {
  /** View window to host: its document is up and wants the frame's document. */
  hello: "plugin-view:hello",
  /** Host to every view window: the host started, so say hello again. */
  host: "plugin-view:host",
  /** Host to view window: the frame's `{ srcdoc, title }`. */
  load: "plugin-view:load",
  /** View window to host: `{ label, data }`, a message from the frame. */
  message: "plugin-view:message",
  /** Host to view window: a message for the frame. */
  post: "plugin-view:post",
  /** View window to host: the frame left its document, so it no longer speaks for the plugin. */
  navigated: "plugin-view:navigated",
  /** Rust to host: the window closed. */
  closed: "plugin-view:closed",
} as const;

export const HOST_WINDOW = "main";
const PREFIX = "view:";

/** Plugin and view ids can't hold `:`, so the label names one view of one plugin. Rust builds the same. */
export const viewLabel = (pluginId: string, viewId: string) => `${PREFIX}${pluginId}:${viewId}`;
export const isViewLabel = (label: string) => label.startsWith(PREFIX);
export function parseViewLabel(label: string): { pluginId: string; viewId: string } | null {
  const [pluginId, viewId, ...rest] = label.slice(PREFIX.length).split(":");
  return isViewLabel(label) && pluginId && viewId && !rest.length ? { pluginId, viewId } : null;
}

/** How the windows reach each other: Tauri events in the app, a stand-in in tests. */
export type Bus = {
  emitTo(label: string, event: string, payload: unknown): unknown;
  /** Sends to every window. */
  emit(event: string, payload: unknown): unknown;
  /** Hears `event` when it's sent to this window (or to every window). Resolves a function that stops listening. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
};

/** What a view window shows: the frame's document and its title. */
export type ViewDocument = { srcdoc: string; title: string };

/** Anything a message can be posted to, like a frame's `contentWindow`. */
export type MessageTarget = { postMessage(message: unknown, targetOrigin: string): void };

/** The host's end: gives each view window its frame and passes messages both ways. */
export class ViewWindowHost {
  private bus: Bus;
  private handlers: {
    /** Resolves the frame's document for a window that asked, or null for one nobody opened. */
    attach: (label: string) => Promise<ViewDocument | null>;
    message: (label: string, data: unknown) => void;
    /** The window closed, or its frame navigated away. */
    detach: (label: string) => void;
  };
  private off: Promise<(() => void)[]>;

  constructor(bus: Bus, handlers: ViewWindowHost["handlers"]) {
    this.bus = bus;
    this.handlers = handlers;
    this.off = Promise.all([
      bus.listen<{ label: string }>(VIEW_EVENTS.hello, ({ label }) => void this.hello(label)),
      bus.listen<{ label: string; data: unknown }>(VIEW_EVENTS.message, ({ label, data }) => handlers.message(label, data)),
      bus.listen<{ label: string }>(VIEW_EVENTS.navigated, ({ label }) => handlers.detach(label)),
      bus.listen<string>(VIEW_EVENTS.closed, (label) => handlers.detach(label)),
    ]);
    // Windows still open from before the host reloaded: the ones it doesn't know get closed by `attach`.
    void this.off.then(() => bus.emit(VIEW_EVENTS.host, null));
  }

  private async hello(label: string) {
    if (!isViewLabel(label)) return;
    const doc = await this.handlers.attach(label);
    if (doc) this.bus.emitTo(label, VIEW_EVENTS.load, doc);
  }

  post(label: string, message: unknown) {
    this.bus.emitTo(label, VIEW_EVENTS.post, message);
  }

  dispose() {
    void this.off.then((offs) => offs.forEach((off) => off()));
  }
}

/**
 * The view window's end. `mount` puts a sandboxed frame with the given document on the page and returns its
 * window; call `loaded` on each of the frame's load events and `receive` with each message the page gets.
 */
export class ViewWindowRelay {
  private label: string;
  private bus: Bus;
  private mount: (doc: ViewDocument) => MessageTarget | null;
  private frame: MessageTarget | null = null;
  private loads = 0;
  private off: Promise<(() => void)[]>;

  constructor(label: string, bus: Bus, mount: (doc: ViewDocument) => MessageTarget | null) {
    this.label = label;
    this.bus = bus;
    this.mount = mount;
    const hello = () => bus.emitTo(HOST_WINDOW, VIEW_EVENTS.hello, { label });
    this.off = Promise.all([
      bus.listen<ViewDocument>(VIEW_EVENTS.load, (doc) => {
        this.loads = 0;
        this.frame = this.mount(doc);
      }),
      bus.listen<unknown>(VIEW_EVENTS.post, (message) => this.frame?.postMessage(message, "*")),
      bus.listen<null>(VIEW_EVENTS.host, hello),
    ]);
    void this.off.then(hello);
  }

  /** The frame's document loads once. A second load means it navigated, so the host lets it go. */
  loaded() {
    if (++this.loads > 1) {
      this.frame = null;
      this.bus.emitTo(HOST_WINDOW, VIEW_EVENTS.navigated, { label: this.label });
    }
  }

  /** Only the frame's own messages go to the host, which checks the nonce in each. */
  receive(source: unknown, data: unknown) {
    if (this.frame && source === this.frame) this.bus.emitTo(HOST_WINDOW, VIEW_EVENTS.message, { label: this.label, data });
  }

  dispose() {
    void this.off.then((offs) => offs.forEach((off) => off()));
  }
}
