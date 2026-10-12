import { Channel, invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

import { api, pagesOf, type McpCall, type PluginTranscriptEvent, type PluginView, type ToolStatus, type ViewPlace } from "@/lib/api";
import { parseViewLabel, viewLabel, ViewWindowHost, type Bus } from "@/lib/plugin-views";
import { requestToolInstall } from "@/lib/tool-install";

/**
 * Runs each plugin in a hidden `<iframe sandbox="allow-scripts">`. The opaque origin keeps it away from
 * the app's DOM and storage, and on macOS and Linux Tauri doesn't inject its IPC bridge into frames, so
 * postMessage is its only way out. Anything that touches the machine is re-checked in Rust against the manifest.
 * On Windows WebView2 runs the IPC bridge in child frames too, so Rust loads no plugins there yet (see `lib.rs`).
 */

export type Tone = "neutral" | "info" | "success" | "warning" | "danger" | "merged";
export type BadgeIcon = "pr-open" | "pr-merged" | "pr-closed" | "pr-draft";
export type CountIcon = "changed" | "push" | "pull";

export type Badge = {
  label: string;
  tone: Tone;
  /** Shows a spinner in the pill while the plugin refreshes. */
  loading?: boolean;
  icon?: BadgeIcon;
  /** Small icon-and-number chips after the label, like commits to push. */
  counts?: { icon: CountIcon; value: number }[];
  title?: string;
  subtitle?: string;
  rows?: { label: string; value: string; tone?: Tone }[];
  actions?: { id: string; label: string; primary?: boolean }[];
};

/**
 * A review comment. `path` is null for a comment on the whole pull request, and `line` is null when the
 * comment no longer matches the diff.
 */
export type DiffComment = {
  id: number;
  replyTo: number | null;
  path: string | null;
  line: number | null;
  side: "additions" | "deletions";
  author: string;
  body: string;
  createdAt: string;
  url: string;
  /** On the first comment of a thread: the viewer offers a reply box, and sends replies to the plugin's `onReply`. */
  canReply: boolean;
};

/** What the diff viewer shows. No `patch` and no `error` means it is still loading. */
export type DiffView = {
  id: string;
  pluginId: string;
  title: string;
  subtitle?: string;
  patch?: string;
  comments?: DiffComment[];
  error?: string;
};

/** Every terminal pane; `session` is set while Claude runs in it. */
export type PluginPane = {
  paneId: string;
  cwd: string;
  command: string;
  project: string;
  session: { sessionId: string; name: string | null; state: string; startedAt: number } | null;
  /** The focused pane of the tab on screen. */
  focused: boolean;
};

/** A new tab, or a split to the right of or below the focused pane. */
export type PanePlacement = "tab" | "right" | "down";
const placements: unknown[] = ["tab", "right", "down"] satisfies PanePlacement[];
/** Where Rust approved a plugin's pane: its project, a folder inside it, and the line to type into its shell. */
export type PanePlan = { spaceId: string; cwd: string | null; input: string | null; placement: PanePlacement };


type Callbacks = {
  setBadge: (pluginId: string, paneId: string, badge: Badge | null) => void;
  openDiff: (view: DiffView) => void;
  updateDiff: (pluginId: string, id: string, update: Partial<DiffView>) => void;
  /** A plugin stopped: drop everything it showed. */
  clearPlugin: (pluginId: string) => void;
  /** Short text next to a sidebar's title bar button, like a running timer; null clears it. */
  setSidebarLabel: (pluginId: string, sidebarId: string, label: SidebarLabel | null) => void;
  /** The project on screen, where a pane without a `cwd` opens. */
  currentProject: () => string | null;
  /** Opens a pane Rust approved; resolves its Rust pane id once its shell runs. */
  openPane: (plan: PanePlan) => Promise<string>;
  /** Shows a pane and moves the keyboard to it. */
  focusPane: (paneId: string) => void;
  /** Shows a plugin page in the sidebar or the modal. The host opens windows itself. */
  showView: (pluginId: string, viewId: string, place: "sidebar" | "modal") => void;
  /** Takes a plugin page out of the sidebar and the modal. */
  hideView: (pluginId: string, viewId: string) => void;
};

type Running = {
  id: string;
  version: string;
  plugin: PluginView;
  ui: string[];
  /** Its sidebar and view ids. */
  views: string[];
  /** `main` runs the plugin's script; the others show one of its pages, `viewId`, or a sidebar's popover page. */
  kind: "main" | ViewPlace | "popover";
  viewId?: string;
  ready: boolean;
  /**
   * Given only to the frame's own document, and required on every message. A frame that navigates keeps its
   * window, so this is what stops a page it navigated to from speaking as the plugin.
   */
  nonce: string;
  /** Sends to the frame: straight to it, or through its view window. */
  post: (message: object) => void;
  /** Removes the frame, or closes its window. */
  remove: () => void;
};

/** Tauri events to and from the other windows, heard only when sent to this one. */
export const tauriBus = (): Bus => {
  const self = getCurrentWebviewWindow().label;
  return {
    emitTo: (label, event, payload) => emitTo(label, event, payload),
    emit: (event, payload) => emit(event, payload),
    listen: <T,>(event: string, handler: (payload: T) => void) => listen<T>(event, (e) => handler(e.payload), { target: self }),
  };
};

/** The first thing in every plugin document: hands the SDK its nonce. */
const nonceScript = (nonce: string) => `<script>window.__wingsNonce=${JSON.stringify(nonce)}</script>`;

const tones: Tone[] = ["neutral", "info", "success", "warning", "danger", "merged"];
const icons: BadgeIcon[] = ["pr-open", "pr-merged", "pr-closed", "pr-draft"];
const countIcons: unknown[] = ["changed", "push", "pull"] satisfies CountIcon[];

const str = (v: unknown, max: number) => (typeof v === "string" ? v.slice(0, max) : undefined);
const tone = (v: unknown): Tone => (tones.includes(v as Tone) ? (v as Tone) : "neutral");

/** Plugin data is untrusted: keep only known fields, bounded in size. */
function cleanBadge(raw: unknown): Badge | null {
  if (!raw || typeof raw !== "object") return null;
  const b = raw as Record<string, unknown>;
  const label = str(b.label, 40);
  if (!label) return null;
  return {
    label,
    tone: tone(b.tone),
    loading: b.loading === true,
    icon: icons.includes(b.icon as BadgeIcon) ? (b.icon as BadgeIcon) : undefined,
    counts: (Array.isArray(b.counts) ? b.counts : []).slice(0, 3).flatMap((c) =>
      countIcons.includes(c?.icon) && Number.isInteger(c?.value) && c.value > 0 ? [{ icon: c.icon as CountIcon, value: Math.min(c.value, 99_999) }] : [],
    ),
    title: str(b.title, 200),
    subtitle: str(b.subtitle, 200),
    rows: (Array.isArray(b.rows) ? b.rows : []).slice(0, 12).flatMap((r) => {
      const label = str(r?.label, 40);
      const value = str(r?.value, 200);
      return label && value ? [{ label, value, tone: r?.tone ? tone(r.tone) : undefined }] : [];
    }),
    actions: (Array.isArray(b.actions) ? b.actions : []).slice(0, 4).flatMap((a) => {
      const id = str(a?.id, 40);
      const label = str(a?.label, 40);
      return id && label ? [{ id, label, primary: a?.primary === true }] : [];
    }),
  };
}

function cleanComments(raw: unknown): DiffComment[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  return raw.slice(0, 1000).flatMap((c) => {
    const id = Number(c?.id);
    const path = c?.path === null ? null : str(c?.path, 500);
    if (!Number.isFinite(id) || path === undefined || path === "") return [];
    return [
      {
        id,
        replyTo: Number.isInteger(c.replyTo) ? c.replyTo : null,
        path,
        line: Number.isInteger(c.line) && c.line > 0 ? c.line : null,
        side: c.side === "deletions" ? "deletions" : "additions",
        author: str(c.author, 80) ?? "unknown",
        body: str(c.body, 20_000) ?? "",
        createdAt: str(c.createdAt, 40) ?? "",
        url: str(c.url, 500)?.startsWith("https://") ? str(c.url, 500)! : "",
        canReply: c.canReply === true,
      },
    ];
  });
}

/** Fields of an openDiff/updateDiff call; the patch is passed through as-is, the viewer only parses it. */
function cleanDiffUpdate(p: Record<string, unknown>): Partial<DiffView> {
  return {
    ...(typeof p.patch === "string" ? { patch: p.patch } : {}),
    ...(p.comments !== undefined ? { comments: cleanComments(p.comments) } : {}),
    ...(typeof p.error === "string" ? { error: p.error.slice(0, 500) } : {}),
  };
}

/**
 * `run` is new for every start. WebKit keeps files from non-http schemes in its memory cache for good, so a
 * reinstalled plugin would otherwise get its old scripts until Wings quits.
 */
const pluginUrl = (id: string, run: string, file: string) =>
  `${/Windows/.test(navigator.userAgent) ? "http://wings-plugin.localhost" : "wings-plugin://localhost"}/${id}/${run}/${file}`;
const newRun = () => crypto.randomUUID().slice(0, 8);

/** A sidebar button's live text, and the rows shown when you point at it. */
export type SidebarLabel = { text: string; rows: { label: string; value: string }[]; tone?: "warning" | "danger" };

export class PluginHost {
  /** Frames in this window by their window, and view windows by label. */
  private frames = new Map<Window | string, Running>();
  /** View windows asked for but not yet showing their page, by label. */
  private opening = new Map<string, { plugin: PluginView; viewId: string }>();
  private windows = new ViewWindowHost(tauriBus(), {
    attach: (label) => this.attachWindow(label),
    message: (label, data) => {
      const running = this.frames.get(label);
      if (running) void this.receive(running, data);
    },
    detach: (label) => this.stop(label),
  });
  private panes: PluginPane[] = [];
  private actions = new Map<string, { pluginId: string; done: () => void; fail: (error: Error) => void }>();
  private nextDiff = 1;
  /** MCP tool calls a plugin is running, by the token it answers with. */
  private tools = new Map<string, { pluginId: string; callId: number }>();
  private unlistenTools = api.onMcpCall((call) => this.runTool(call));
  /** `openPane` calls waiting for their new pane to reach plugins, by Rust pane id. */
  private paneWaiters = new Map<string, () => void>();
  private unlistenTranscript = api.onPluginTranscript((events) => this.publishTranscript(events));

  constructor(private callbacks: Callbacks) {
    window.addEventListener("message", this.onMessage);
  }

  /**
   * Runs the plugins that are on and approved and stops the rest, without a restart. A plugin whose version
   * changed, or the one named in `restart` (just reinstalled), starts again from scratch.
   */
  sync(plugins: PluginView[], restart?: string) {
    const wanted = new Map(plugins.filter((p) => p.enabled && p.approved).map((p) => [p.id, p]));
    // Each frame on its own: a plugin has a main frame and maybe page frames and windows, all of which stay.
    for (const [key, running] of this.frames) {
      const next = wanted.get(running.id);
      if (!next || next.version !== running.version || running.id === restart) this.stop(key);
    }
    const live = new Set([...this.frames.values()].filter((r) => r.kind === "main").map((r) => r.id));
    for (const plugin of wanted.values()) if (!live.has(plugin.id)) this.run(plugin);
  }

  private run(plugin: PluginView) {
    const sdk = new URL("/plugin-sdk.js", location.href).href;
    const frame = document.createElement("iframe");
    frame.sandbox.add("allow-scripts");
    frame.hidden = true;
    frame.title = `Plugin ${plugin.name}`;
    const nonce = crypto.randomUUID();
    frame.srcdoc = `<!doctype html><meta charset="utf-8">${nonceScript(nonce)}<script src="${sdk}"></script><script src="${pluginUrl(plugin.id, newRun(), plugin.main)}"></script>`;
    document.body.appendChild(frame);
    this.track(frame, plugin, "main", nonce);
  }

  private entry(plugin: PluginView, kind: Running["kind"], nonce: string, viewId: string | undefined, port: Pick<Running, "post" | "remove">): Running {
    return { id: plugin.id, version: plugin.version, plugin, ui: plugin.contributes.ui, views: pagesOf(plugin).map((v) => v.id), kind, viewId, ready: false, nonce, ...port };
  }

  private track(frame: HTMLIFrameElement, plugin: PluginView, kind: Running["kind"], nonce: string, viewId?: string) {
    const win = frame.contentWindow;
    if (!win) return;
    const running = this.entry(plugin, kind, nonce, viewId, { post: (message) => this.post(win, message), remove: () => frame.remove() });
    this.frames.set(win, running);
    // The plugin's own document loads once. A second load means it navigated away, so let it go.
    let loads = 0;
    frame.addEventListener("load", () => {
      if (++loads > 1 && this.frames.get(win) === running) this.stop(win);
    });
  }

  /**
   * A page's document: the plugin's HTML with its nonce, the SDK and Wings' base styles loaded first. With
   * `popover`, the file is the sidebar's popover page instead of the sidebar's own.
   */
  private async pageDocument(plugin: PluginView, viewId: string, popover = false) {
    const page = pagesOf(plugin).find((v) => v.id === viewId);
    if (!page) throw new Error(`${plugin.id} has no view ${viewId}`);
    const file = popover ? plugin.contributes.sidebars.find((s) => s.id === viewId)?.popover?.page : page.page;
    if (!file) throw new Error(`${plugin.id} has no popover for ${viewId}`);
    const run = newRun();
    const html = await (await fetch(pluginUrl(plugin.id, run, file))).text();
    const base = pluginUrl(plugin.id, run, file.includes("/") ? file.slice(0, file.lastIndexOf("/") + 1) : "");
    const nonce = crypto.randomUUID();
    const head = `<meta charset="utf-8">${nonceScript(nonce)}<base href="${base}"><link rel="stylesheet" href="${new URL("/plugin-ui.css", location.href).href}"><script src="${new URL("/plugin-sdk.js", location.href).href}"></script>`;
    const srcdoc = /<head[^>]*>/i.test(html) ? html.replace(/<head[^>]*>/i, (tag) => tag + head) : `<!doctype html><html><head>${head}</head><body>${html}</body></html>`;
    return { page, srcdoc, nonce };
  }

  /**
   * Shows one of a plugin's pages inside `container`, sandboxed like the plugin itself. A sidebar frame lives
   * until the plugin stops, so hiding the sidebar keeps its state. Resolves a function that removes the frame.
   */
  async mountPage(plugin: PluginView, viewId: string, container: HTMLElement, kind: "sidebar" | "modal" | "popover") {
    const { page, srcdoc, nonce } = await this.pageDocument(plugin, viewId, kind === "popover");
    const frame = document.createElement("iframe");
    frame.sandbox.add("allow-scripts", "allow-forms");
    frame.title = page.title;
    frame.className = "size-full border-0 bg-transparent";
    frame.srcdoc = srcdoc;
    container.replaceChildren(frame);
    this.track(frame, plugin, kind, nonce, viewId);
    const win = frame.contentWindow;
    return () => void (win && this.stop(win));
  }

  /** Opens a plugin page in a window of its own, or brings its window to the front. */
  async openWindow(plugin: PluginView, viewId: string) {
    const label = viewLabel(plugin.id, viewId);
    // Before Rust opens it, so the window's hello can't arrive first.
    this.opening.set(label, { plugin, viewId });
    try {
      await invoke("plugin_view_open", { pluginId: plugin.id, viewId });
    } catch (error) {
      this.opening.delete(label);
      throw error;
    }
  }

  /** A view window asked for its page. One nobody opened, like one left from before a reload, is closed. */
  private async attachWindow(label: string) {
    const known = this.frames.get(label);
    const target = this.opening.get(label) ?? (known && known.viewId ? { plugin: known.plugin, viewId: known.viewId } : undefined);
    this.opening.delete(label);
    if (!target) {
      const ids = parseViewLabel(label);
      if (ids) void invoke("plugin_view_close", ids);
      return null;
    }
    // A window that says hello again has a new document, so its old frame is gone.
    if (known) this.frames.delete(label);
    const { page, srcdoc, nonce } = await this.pageDocument(target.plugin, target.viewId);
    const { id: pluginId } = target.plugin;
    this.frames.set(
      label,
      this.entry(target.plugin, "window", nonce, target.viewId, {
        post: (message) => this.windows.post(label, { wings: 1, ...message }),
        remove: () => void invoke("plugin_view_close", { pluginId, viewId: target.viewId }),
      }),
    );
    return { srcdoc, title: page.title };
  }

  /** Closes a page wherever it's open. */
  private closeView(pluginId: string, viewId: string) {
    this.callbacks.hideView(pluginId, viewId);
    this.stop(viewLabel(pluginId, viewId));
  }

  private stop(key: Window | string) {
    const running = this.frames.get(key);
    if (!running) return;
    running.remove();
    this.frames.delete(key);
    if (running.kind !== "main") return;
    for (const [token, action] of this.actions) if (action.pluginId === running.id) this.settle(token, new Error("The plugin was turned off"));
    for (const [token, tool] of this.tools) {
      if (tool.pluginId !== running.id) continue;
      this.tools.delete(token);
      void api.mcpToolResult(tool.callId, `${running.id} was turned off in Wings`, true);
    }
    this.callbacks.clearPlugin(running.id);
  }

  publishPanes(panes: PluginPane[]) {
    this.panes = panes;
    for (const plugin of this.frames.values()) if (plugin.ready) plugin.post({ event: "panes", data: panes });
    for (const [paneId, done] of this.paneWaiters) if (panes.some((p) => p.paneId === paneId)) done();
  }

  /** Resolves once plugins can see the pane in `onPanes`, so a call on it right after `openPane` works. */
  private listed(paneId: string) {
    return new Promise<void>((resolve) => {
      if (this.panes.some((p) => p.paneId === paneId)) return resolve();
      const done = () => {
        clearTimeout(timer);
        this.paneWaiters.delete(paneId);
        resolve();
      };
      // A pane closed before it was listed never will be, so don't wait on it for good.
      const timer = setTimeout(done, 5_000);
      this.paneWaiters.set(paneId, done);
    });
  }

  /** Each entry goes to the pages of the plugins Rust found may read its type, if they're running. */
  private publishTranscript(events: PluginTranscriptEvent[]) {
    for (const { plugins, ...data } of events) {
      for (const plugin of this.frames.values()) if (plugin.ready && plugins.includes(plugin.id)) plugin.post({ event: "transcript", data });
    }
  }

  /** Resolves when the plugin has finished handling the action, so the button can show progress. */
  sendAction(pluginId: string, paneId: string, actionId: string): Promise<void> {
    return this.ask(pluginId, "action", { paneId, actionId });
  }

  /** Hands a reply typed in the diff viewer to the plugin; resolves once it's posted, or fails with the plugin's error. */
  sendReply(pluginId: string, diffId: string, replyTo: number, body: string): Promise<void> {
    return this.ask(pluginId, "reply", { diffId, replyTo, body });
  }

  private ask(pluginId: string, event: string, data: object): Promise<void> {
    const entry = [...this.frames].find(([, p]) => p.id === pluginId && p.kind === "main");
    if (!entry) return Promise.reject(new Error(`plugin ${pluginId} is not running`));
    const token = crypto.randomUUID();
    return new Promise<void>((done, fail) => {
      // Just over the 5 min exec cap, so an action waiting on a long exec (like a sign-in) can finish.
      const timer = setTimeout(() => this.settle(token, new Error("The plugin didn't respond")), 310_000);
      this.actions.set(token, {
        pluginId,
        done: () => (clearTimeout(timer), done()),
        fail: (e) => (clearTimeout(timer), fail(e)),
      });
      entry[1].post({ event, data: { ...data, token } });
    });
  }

  /** Hands an MCP tool call from Claude to the plugin's main frame. Rust has checked the tool is declared. */
  private runTool(call: McpCall) {
    const entry = [...this.frames].find(([, p]) => p.id === call.pluginId && p.kind === "main" && p.ready);
    if (!entry) return void api.mcpToolResult(call.callId, `${call.pluginId} is still starting in Wings. Try again in a moment.`, true);
    const token = crypto.randomUUID();
    this.tools.set(token, { pluginId: call.pluginId, callId: call.callId });
    entry[1].post({ event: "tool", data: { token, name: call.tool, arguments: call.arguments, paneId: call.paneId } });
  }

  private settle(token: string, error: Error | null) {
    const action = this.actions.get(token);
    this.actions.delete(token);
    if (error) action?.fail(error);
    else action?.done();
  }

  dispose() {
    window.removeEventListener("message", this.onMessage);
    void this.unlistenTools.then((off) => off());
    void this.unlistenTranscript.then((off) => off());
    for (const running of this.frames.values()) running.remove();
    this.frames.clear();
    this.windows.dispose();
  }

  // Sandboxed frames have an opaque origin, which can't be named as a target.
  private post(win: Window, message: object) {
    win.postMessage({ wings: 1, ...message }, "*");
  }

  private onMessage = (event: MessageEvent) => {
    const plugin = event.source ? this.frames.get(event.source as Window) : undefined;
    if (plugin) void this.receive(plugin, event.data);
  };

  /** A message from a plugin frame in this window or in a view window. */
  private async receive(plugin: Running, data: unknown) {
    if (!data || typeof data !== "object") return;
    const msg = data as Record<string, unknown>;
    if (msg.wings !== 1 || typeof msg.id !== "number" || msg.nonce !== plugin.nonce) return;
    const id = msg.id;
    try {
      const output = (line: string) => plugin.post({ output: { call: id, line } });
      const params = msg.params && typeof msg.params === "object" ? (msg.params as Record<string, unknown>) : {};
      const result = await this.handle(plugin, String(msg.method), params, output);
      plugin.post({ id, result });
      if (msg.method === "ready") plugin.post({ event: "panes", data: this.panes });
    } catch (error) {
      plugin.post({ id, error: String(error instanceof Error ? error.message : error) });
    }
  }

  private async handle(plugin: Running, method: string, p: Record<string, unknown>, output: (line: string) => void) {
    const pluginId = plugin.id;
    // The manager lists what a plugin adds from its manifest, so it can't draw anything it didn't declare.
    const needs = (kind: string) => {
      if (!plugin.ui.includes(kind)) throw new Error(`${pluginId} must declare "${kind}" in contributes.ui`);
    };
    switch (method) {
      case "ready":
        plugin.ready = true;
        return null;
      case "exec": {
        const onOutput = new Channel<string>(output);
        return invoke("plugin_exec", {
          pluginId,
          request: {
            program: String(p.program),
            args: Array.isArray(p.args) ? p.args.map(String) : [],
            cwd: typeof p.cwd === "string" ? p.cwd : null,
            timeoutMs: typeof p.timeoutMs === "number" ? p.timeoutMs : null,
            stream: p.stream === true,
          },
          onOutput,
        });
      }
      case "transcript":
        return invoke("plugin_transcript", {
          pluginId,
          sessionId: String(p.sessionId),
          types: Array.isArray(p.types) ? p.types.map(String) : [],
          last: typeof p.last === "number" && p.last >= 1 ? Math.floor(p.last) : null,
        });
      case "statusline":
        return invoke("plugin_statusline", { pluginId });
      case "ports":
        return invoke("plugin_ports", { pluginId });
      case "openUrl":
        return invoke("plugin_open_url", { pluginId, url: String(p.url) });
      case "post":
        return invoke("plugin_post", {
          pluginId,
          request: {
            program: String(p.program),
            url: String(p.url),
            host: typeof p.host === "string" ? p.host : null,
            fields: p.fields && typeof p.fields === "object" && !Array.isArray(p.fields) ? p.fields : {},
          },
        });
      case "openPane": {
        const placement = p.placement ?? "tab";
        if (!placements.includes(placement)) throw new Error('openPane placement is "tab", "right" or "down"');
        const text = (key: string) => {
          const value = p[key];
          if (value == null) return null;
          if (typeof value !== "string") throw new Error(`openPane ${key} must be a string`);
          return value;
        };
        // Rust checks the command and folder; the pane is then opened here, where tabs and splits live.
        const plan = await invoke<Omit<PanePlan, "placement">>("plugin_open_pane", {
          pluginId,
          request: { command: text("command"), cwd: text("cwd"), spaceId: this.callbacks.currentProject() },
        });
        const paneId = await this.callbacks.openPane({ ...plan, placement: placement as PanePlacement });
        await this.listed(paneId);
        return { paneId };
      }
      case "focusPane": {
        const paneId = String(p.paneId);
        await invoke("plugin_focus_pane", { pluginId, paneId });
        this.callbacks.focusPane(paneId);
        return null;
      }
      case "installTool": {
        // Rust checks the plugin runs it and Wings knows it; the command shown and run comes from Rust's table.
        const tool = await invoke<ToolStatus>("plugin_tool", { pluginId, program: String(p.program) });
        return requestToolInstall({ pluginId, pluginName: plugin.plugin.name, tool });
      }
      case "notify":
        return invoke("plugin_notify", {
          pluginId,
          title: typeof p.title === "string" ? p.title : "",
          body: typeof p.body === "string" ? p.body : "",
        });
      case "fetch": {
        const headers = p.headers && typeof p.headers === "object" ? (p.headers as Record<string, unknown>) : {};
        return invoke("plugin_fetch", {
          pluginId,
          request: {
            url: String(p.url),
            method: str(p.method, 10) ?? null,
            headers: Object.fromEntries(Object.entries(headers).map(([k, v]) => [k, String(v)])),
            body: typeof p.body === "string" ? p.body : null,
            bearer: str(p.bearer, 64) ?? null,
          },
        });
      }
      case "secretSet":
        return invoke("plugin_secret_set", { pluginId, name: String(p.name), value: String(p.value) });
      case "secretDelete":
        return invoke("plugin_secret_delete", { pluginId, name: String(p.name) });
      case "secretHas":
        return invoke("plugin_secret_has", { pluginId, name: String(p.name) });
      case "storageGet":
        return invoke("plugin_storage_get", { pluginId, key: String(p.key) });
      case "storageSet": {
        // Values are what JSON.stringify keeps. Rust checks the 1 MB total; this only stops one value that
        // can't fit from crossing the bridge.
        const text = JSON.stringify(p.value);
        if (text === undefined) throw new Error("wings.storage.set needs a JSON value. Use delete to remove a key");
        if (text.length > 1024 * 1024) throw new Error("Storage is limited to 1 MB per plugin");
        return invoke("plugin_storage_set", { pluginId, key: String(p.key), value: JSON.parse(text) });
      }
      case "storageDelete":
        return invoke("plugin_storage_delete", { pluginId, key: String(p.key) });
      case "storageKeys":
        return invoke("plugin_storage_keys", { pluginId });
      case "setSidebarLabel": {
        const sidebarId = String(p.sidebarId);
        if (!plugin.views.includes(sidebarId)) throw new Error(`${pluginId} has no sidebar or view ${sidebarId}`);
        // A bound on untrusted data, not the display width: the title bar cuts it with an ellipsis.
        const text = p.label === null ? undefined : str(p.label, 80);
        const rows = (Array.isArray(p.rows) ? p.rows : []).slice(0, 6).flatMap((r) => {
          const label = str(r?.label, 40);
          const value = str(r?.value, 120);
          return label && value ? [{ label, value }] : [];
        });
        const tone = p.tone === "warning" || p.tone === "danger" ? p.tone : undefined;
        this.callbacks.setSidebarLabel(pluginId, sidebarId, text ? { text, rows, tone } : null);
        return null;
      }
      case "broadcast": {
        // Only the plugin's own pages hear it, wherever they're open, so its main script and pages can keep each other current.
        const text = JSON.stringify(p.message ?? null);
        if (text.length > 64_000) throw new Error("A broadcast is limited to 64 KB");
        for (const other of this.frames.values()) {
          if (other.id === pluginId && other !== plugin && other.ready) other.post({ event: "broadcast", data: JSON.parse(text) });
        }
        return null;
      }
      case "openView": {
        const page = pagesOf(plugin.plugin).find((v) => v.id === p.id);
        if (!page) throw new Error(`${pluginId} has no view ${String(p.id)}`);
        const place = page.opens.find((o) => o === (p.as ?? page.opens[0]));
        if (!place) throw new Error(`${page.id} opens as ${page.opens.join(" or ")}, not ${String(p.as)}`);
        if (place === "window") await this.openWindow(plugin.plugin, page.id);
        else this.callbacks.showView(pluginId, page.id, place);
        return null;
      }
      case "closeView": {
        const viewId = String(p.id);
        if (!plugin.views.includes(viewId)) throw new Error(`${pluginId} has no view ${viewId}`);
        this.closeView(pluginId, viewId);
        return null;
      }
      case "setBadge": {
        needs("badges");
        const paneId = String(p.paneId);
        if (!this.panes.some((s) => s.paneId === paneId)) throw new Error(`no pane ${paneId}`);
        this.callbacks.setBadge(pluginId, paneId, p.badge === null ? null : cleanBadge(p.badge));
        return null;
      }
      case "actionDone":
        this.settle(String(p.token), typeof p.error === "string" ? new Error(p.error) : null);
        return null;
      case "toolDone": {
        const tool = this.tools.get(String(p.token));
        if (!tool || tool.pluginId !== pluginId) throw new Error("unknown tool call");
        this.tools.delete(String(p.token));
        // About 25k tokens, Claude Code's default cap on what one MCP tool returns.
        return api.mcpToolResult(tool.callId, String(p.text ?? "").slice(0, 100_000), p.isError === true);
      }
      case "openDiff": {
        needs("diff");
        const title = str(p.title, 200);
        if (!title) throw new Error("openDiff needs a title");
        const id = `${pluginId}:${this.nextDiff++}`;
        this.callbacks.openDiff({ id, pluginId, title, subtitle: str(p.subtitle, 200), ...cleanDiffUpdate(p) });
        return { id };
      }
      case "updateDiff":
        needs("diff");
        this.callbacks.updateDiff(pluginId, String(p.id), cleanDiffUpdate(p));
        return null;
      default:
        throw new Error(`unknown method ${method}`);
    }
  }
}
