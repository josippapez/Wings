import { Channel, invoke } from "@tauri-apps/api/core";

import type { PluginView } from "@/lib/api";

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
  session: { sessionId: string; name: string | null; state: string } | null;
};


type Callbacks = {
  setBadge: (pluginId: string, paneId: string, badge: Badge | null) => void;
  openDiff: (view: DiffView) => void;
  updateDiff: (pluginId: string, id: string, update: Partial<DiffView>) => void;
  /** A plugin stopped: drop everything it showed. */
  clearPlugin: (pluginId: string) => void;
  /** Short text next to a sidebar's title bar button, like a running timer; null clears it. */
  setSidebarLabel: (pluginId: string, sidebarId: string, label: string | null) => void;
};

type Running = {
  id: string;
  version: string;
  ui: string[];
  sidebars: string[];
  /** `main` runs the plugin's script; a sidebar frame shows one of its pages. */
  kind: "main" | "sidebar";
  ready: boolean;
  frame: HTMLIFrameElement;
};

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

const pluginUrl = (id: string, file: string) =>
  `${/Windows/.test(navigator.userAgent) ? "http://wings-plugin.localhost" : "wings-plugin://localhost"}/${id}/${file}`;

export class PluginHost {
  private frames = new Map<Window, Running>();
  private panes: PluginPane[] = [];
  private actions = new Map<string, { pluginId: string; done: () => void; fail: (error: Error) => void }>();
  private nextDiff = 1;

  constructor(private callbacks: Callbacks) {
    window.addEventListener("message", this.onMessage);
  }

  /**
   * Runs the plugins that are on and approved and stops the rest, without a restart. A plugin whose version
   * changed, or the one named in `restart` (just reinstalled), starts again from scratch.
   */
  sync(plugins: PluginView[], restart?: string) {
    const wanted = new Map(plugins.filter((p) => p.enabled && p.approved).map((p) => [p.id, p]));
    for (const [win, running] of this.frames) {
      const next = wanted.get(running.id);
      if (next && next.version === running.version && running.id !== restart) wanted.delete(running.id);
      else this.stop(win);
    }
    for (const plugin of wanted.values()) this.run(plugin);
  }

  private run(plugin: PluginView) {
    const sdk = new URL("/plugin-sdk.js", location.href).href;
    const frame = document.createElement("iframe");
    frame.sandbox.add("allow-scripts");
    frame.hidden = true;
    frame.title = `Plugin ${plugin.name}`;
    frame.srcdoc = `<!doctype html><meta charset="utf-8"><script src="${sdk}"></script><script src="${pluginUrl(plugin.id, plugin.main)}"></script>`;
    document.body.appendChild(frame);
    this.track(frame, plugin, "main");
  }

  private track(frame: HTMLIFrameElement, plugin: PluginView, kind: Running["kind"]) {
    if (!frame.contentWindow) return;
    this.frames.set(frame.contentWindow, {
      id: plugin.id,
      version: plugin.version,
      ui: plugin.contributes.ui,
      sidebars: plugin.contributes.sidebars.map((s) => s.id),
      kind,
      ready: false,
      frame,
    });
  }

  /**
   * Shows one of a plugin's sidebar pages inside `container`, sandboxed like the plugin itself, with the SDK
   * and Wings' base styles loaded first. The frame lives until the plugin stops, so hiding the sidebar keeps
   * its state.
   */
  async mountSidebar(plugin: PluginView, sidebarId: string, container: HTMLElement) {
    const sidebar = plugin.contributes.sidebars.find((s) => s.id === sidebarId);
    if (!sidebar) throw new Error(`${plugin.id} has no sidebar ${sidebarId}`);
    const html = await (await fetch(pluginUrl(plugin.id, sidebar.page))).text();
    const base = pluginUrl(plugin.id, sidebar.page.includes("/") ? sidebar.page.slice(0, sidebar.page.lastIndexOf("/") + 1) : "");
    const head = `<meta charset="utf-8"><base href="${base}"><link rel="stylesheet" href="${new URL("/plugin-ui.css", location.href).href}"><script src="${new URL("/plugin-sdk.js", location.href).href}"></script>`;
    const frame = document.createElement("iframe");
    frame.sandbox.add("allow-scripts", "allow-forms");
    frame.title = sidebar.title;
    frame.className = "size-full border-0 bg-transparent";
    frame.srcdoc = /<head[^>]*>/i.test(html) ? html.replace(/<head[^>]*>/i, (tag) => tag + head) : `<!doctype html><html><head>${head}</head><body>${html}</body></html>`;
    container.replaceChildren(frame);
    this.track(frame, plugin, "sidebar");
  }

  private stop(win: Window) {
    const running = this.frames.get(win);
    if (!running) return;
    running.frame.remove();
    this.frames.delete(win);
    for (const [token, action] of this.actions) if (action.pluginId === running.id) this.settle(token, new Error("The plugin was turned off"));
    this.callbacks.clearPlugin(running.id);
  }

  publishPanes(panes: PluginPane[]) {
    this.panes = panes;
    for (const [win, plugin] of this.frames) if (plugin.ready) this.post(win, { event: "panes", data: panes });
  }

  /** Resolves when the plugin has finished handling the action, so the button can show progress. */
  sendAction(pluginId: string, paneId: string, actionId: string): Promise<void> {
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
      this.post(entry[0], { event: "action", data: { paneId, actionId, token } });
    });
  }

  private settle(token: string, error: Error | null) {
    const action = this.actions.get(token);
    this.actions.delete(token);
    if (error) action?.fail(error);
    else action?.done();
  }

  dispose() {
    window.removeEventListener("message", this.onMessage);
    for (const { frame } of this.frames.values()) frame.remove();
    this.frames.clear();
  }

  // Sandboxed frames have an opaque origin, which can't be named as a target.
  private post(win: Window, message: object) {
    win.postMessage({ wings: 1, ...message }, "*");
  }

  private onMessage = async (event: MessageEvent) => {
    const plugin = event.source ? this.frames.get(event.source as Window) : undefined;
    const msg = event.data;
    if (!plugin || msg?.wings !== 1 || typeof msg.id !== "number") return;
    const win = event.source as Window;
    try {
      const output = (line: string) => this.post(win, { output: { call: msg.id, line } });
      const result = await this.handle(plugin, msg.method, msg.params ?? {}, output);
      this.post(win, { id: msg.id, result });
      if (msg.method === "ready") this.post(win, { event: "panes", data: this.panes });
    } catch (error) {
      this.post(win, { id: msg.id, error: String(error instanceof Error ? error.message : error) });
    }
  };

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
        });
      case "openUrl":
        return invoke("plugin_open_url", { pluginId, url: String(p.url) });
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
      case "setSidebarLabel": {
        const sidebarId = String(p.sidebarId);
        if (!plugin.sidebars.includes(sidebarId)) throw new Error(`${pluginId} has no sidebar ${sidebarId}`);
        this.callbacks.setSidebarLabel(pluginId, sidebarId, p.label === null ? null : (str(p.label, 16) ?? null));
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
