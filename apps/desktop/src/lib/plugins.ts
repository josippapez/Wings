import { Channel, invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

import { api, pagesOf, type AgentKind, type McpCall, type PluginTranscriptEvent, type PluginView, type ToolStatus, type ViewPlace } from "@/lib/api";
import { blockStateJson, clampBlockHeight, findBlock, MAX_LIVE_BLOCKS, type BlockMount } from "@/lib/plugin-blocks";
import { boundedJson, cleanNewTab, requireUi, TAB_STATE_MAX, VIEW_ARGS_MAX } from "@/lib/plugin-pages";
import { cleanPaneRail, type PaneRail } from "@/lib/pane-rail";
import { cleanPaneStatus, type PaneStatus } from "@/lib/pane-status";
import { cleanFocus, type DiffFocus } from "@/lib/diff-focus";
import { cleanButtons, type NotificationButton } from "@/lib/notifications";
import { cleanSectionRows, type ProjectSectionRow } from "@/lib/project-sections";
import { FocusTracker } from "@/lib/plugin-focus";
import { gitListeners } from "@/lib/plugin-git";
import { parseViewLabel, viewLabel, ViewWindowHost, type Bus } from "@/lib/plugin-views";
import { requestToolInstall } from "@/lib/tool-install";
import pluginUiCss from "wings-plugin-ui/plugin-ui.css?raw";

/**
 * Runs each plugin in a hidden `<iframe sandbox="allow-scripts">`. The opaque origin keeps it away from
 * the app's DOM and storage, and on macOS and Linux Tauri doesn't inject its IPC bridge into frames, so
 * postMessage is its only way out. Anything that touches the machine is re-checked in Rust against the manifest.
 * On Windows WebView2 runs the IPC bridge in child frames too, so Rust loads no plugins there yet (see `lib.rs`).
 */

export type Tone = "neutral" | "info" | "success" | "warning" | "danger" | "merged";
export type BadgeIcon = "pr-open" | "pr-merged" | "pr-closed" | "pr-draft";
export type CountIcon = "changed" | "push" | "pull";

export type { PaneRail, RailItem } from "@/lib/pane-rail";
export type { PaneStatus, StatusItem } from "@/lib/pane-status";

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
  /** The repo or folder the diff is of; the review's Send to agent offers the agent working there first. */
  cwd?: string;
  patch?: string;
  comments?: DiffComment[];
  /** Where the viewer scrolls to and briefly highlights once the patch is shown, even when it arrives later through `updateDiff`. */
  focus?: DiffFocus;
  error?: string;
};

/** Every terminal pane; `session` is set while an agent (Claude Code or opencode) runs in it. */
export type PluginPane = {
  paneId: string;
  cwd: string;
  command: string;
  project: string;
  /** The id of that project, as `wings.project().id` gives it. */
  projectId: string;
  session: { sessionId: string; kind: AgentKind; name: string | null; state: string; startedAt: number } | null;
  /** The focused pane of the tab on screen. */
  focused: boolean;
};

/** The project a page is about: `wings.project()`. */
export type PluginProject = { id: string; name: string; path: string; branch: string | null };
/** Where a page shows, as `wings.placement()` reports it. */
export type PagePlacement = "sidebar" | "tab" | "modal" | "window" | "block";
/** A page open as a tab: its tab, and the project the tab belongs to. */
export type PageTab = { tabId: string; spaceId: string; args?: unknown };

/** A new tab, or a split to the right of or below the focused pane. */
export type PanePlacement = "tab" | "right" | "down";
const placements: unknown[] = ["tab", "right", "down"] satisfies PanePlacement[];
/** Where Rust approved a plugin's pane: its project, a folder inside it, and the line to type into its shell. */
export type PanePlan = { spaceId: string; cwd: string | null; input: string | null; placement: PanePlacement };


type Callbacks = {
  setBadge: (pluginId: string, paneId: string, badge: Badge | null) => void;
  /** The strip a plugin shows under a pane's terminal; null removes it. */
  setPaneStatus: (pluginId: string, paneId: string, status: PaneStatus | null) => void;
  setPaneRail: (pluginId: string, paneId: string, rail: PaneRail | null) => void;
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
  /**
   * Shows a plugin page in the sidebar, the modal or a tab of the project on screen. The host opens windows itself.
   * For a tab, `multiple` always opens a new one, `newTab` does so for any view, and `args` is what the page reads with `viewArgs`.
   */
  showView: (pluginId: string, viewId: string, place: "sidebar" | "modal" | "tab", tab?: { multiple: boolean; args?: unknown; newTab?: boolean }) => void;
  /** Back or forward in the places the user has been, as the mouse's side buttons do. */
  navigate: (direction: "back" | "forward") => void;
  /** The project with this id, with its branch as it is now. */
  projectOf: (spaceId: string | null) => PluginProject | null;
  /** Every project, with its branch as it is now. */
  projects: () => PluginProject[];
  /** A plugin set, or with null cleared, one of its sections under a project in the sidebar. */
  setProjectSection: (pluginId: string, projectId: string, sectionId: string, rows: ProjectSectionRow[] | null) => void;
  /** The state a view tab saved with `setTabState`, if the tab is still open. */
  tabState: (tabId: string) => unknown;
  setTabState: (tabId: string, state: unknown) => void;
  /** Takes a plugin page out of the sidebar, the modal and its tabs. */
  hideView: (pluginId: string, viewId: string) => void;
  /** Opens Wings Settings at the plugin's own settings page. */
  openSettings: (pluginId: string) => void;
  /** A plugin's notification went out; the notification list keeps it. */
  notified: (pluginName: string, title: string, body: string, plugin: { id: string; projectId: string | null; buttons: NotificationButton[] }) => void;
};

type Running = {
  id: string;
  version: string;
  plugin: PluginView;
  ui: string[];
  /** Its sidebar and view ids. */
  views: string[];
  /** `main` runs the plugin's script; the others show one of its pages, `viewId`, a sidebar's popover page or its settings page. */
  kind: "main" | ViewPlace | "popover" | "settings" | "block";
  viewId?: string;
  ready: boolean;
  /** Set on a page open as a tab: its tab and project. */
  tab?: PageTab;
  /** Set on a page shown as a block on a Project page: the project it is fixed to, and its saved state. */
  block?: BlockMount;
  /** A block, tab, sidebar or popover page that is out of view: its `focus` event says not focused. */
  hidden?: boolean;
  /** The project this frame last learned of, as JSON, once it asked with `project`; changes are sent from then on. */
  lastProject?: string;
  /** The project list this frame last learned of, as JSON, once it asked with `projects`. */
  lastProjects?: string;
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
const nonceScript = (nonce: string, placement: PagePlacement | null, args?: unknown, focused?: boolean) =>
  // `<` is escaped so JSON in the args can't close the script tag.
  `<script>window.__wingsNonce=${JSON.stringify(nonce)};window.__wingsInit=${JSON.stringify({ placement, args: args ?? null, focused }).replace(/</g, "\\u003c")}</script>`;

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
  const focus = cleanFocus(p.focus);
  return {
    ...(typeof p.patch === "string" ? { patch: p.patch } : {}),
    ...(p.comments !== undefined ? { comments: cleanComments(p.comments) } : {}),
    ...(focus ? { focus } : {}),
    ...(typeof p.error === "string" ? { error: p.error.slice(0, 500) } : {}),
  };
}

/**
 * `run` is new for every start. WebKit keeps files from non-http schemes in its memory cache for good, so a
 * reinstalled plugin would otherwise get its old scripts until Wings quits. Pages reuse their plugin's run: a new
 * one per page load would leave another copy of the page's files in that cache each time a page opens.
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
  /** The `run` of each running plugin's start, by plugin id. */
  private runs = new Map<string, string>();
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
  private unlistenGit = api.onGitChanged((projectIds) => this.publishGit(projectIds));
  /** Starts true: Wings opens in front, and the document is checked for focus. */
  private focus = new FocusTracker(document.hasFocus(), (focused) => {
    for (const plugin of this.frames.values()) if (plugin.ready) plugin.post({ event: "focus", data: { focused: focused && !plugin.hidden } });
  });
  private unlistenFocus = api.onWingsFocus((focused) => this.focus.report(focused));

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
    const run = newRun();
    this.runs.set(plugin.id, run);
    frame.srcdoc = `<!doctype html><meta charset="utf-8">${nonceScript(nonce, null)}<script src="${sdk}"></script><script src="${pluginUrl(plugin.id, run, plugin.main)}"></script>`;
    document.body.appendChild(frame);
    this.track(frame, plugin, "main", nonce);
  }

  private entry(plugin: PluginView, kind: Running["kind"], nonce: string, viewId: string | undefined, port: Pick<Running, "post" | "remove">, tab?: PageTab, block?: BlockMount): Running {
    return { id: plugin.id, version: plugin.version, plugin, ui: plugin.contributes.ui, views: pagesOf(plugin).map((v) => v.id), kind, viewId, ready: false, tab, block, nonce, ...port };
  }

  private track(frame: HTMLIFrameElement, plugin: PluginView, kind: Running["kind"], nonce: string, viewId?: string, tab?: PageTab, block?: BlockMount) {
    const win = frame.contentWindow;
    if (!win) return;
    const running = this.entry(plugin, kind, nonce, viewId, { post: (message) => this.post(win, message), remove: () => frame.remove() }, tab, block);
    this.frames.set(win, running);
    // The plugin's own document loads once. A second load means it navigated away, so let it go.
    let loads = 0;
    frame.addEventListener("load", () => {
      if (++loads > 1 && this.frames.get(win) === running) this.stop(win);
    });
  }

  /** The file and title of a page: a sidebar or view by `viewId`, a sidebar's popover, or the plugin's settings page. */
  private pageFile(plugin: PluginView, viewId: string | null, kind: "page" | "popover" | "settings" | "block") {
    if (kind === "block") {
      const block = findBlock(plugin, String(viewId));
      if (!block) throw new Error(`${plugin.id} has no block ${viewId}`);
      return { title: block.title, file: block.page };
    }
    if (kind === "settings") {
      const file = plugin.contributes.settings?.page;
      if (!file) throw new Error(`${plugin.id} has no settings page`);
      return { title: `${plugin.name} settings`, file };
    }
    const page = pagesOf(plugin).find((v) => v.id === viewId);
    if (!page) throw new Error(`${plugin.id} has no view ${viewId}`);
    const file = kind === "popover" ? plugin.contributes.sidebars.find((s) => s.id === viewId)?.popover?.page : page.page;
    if (!file) throw new Error(`${plugin.id} has no popover for ${viewId}`);
    return { title: page.title, file };
  }

  /** A page's document: the plugin's HTML with its nonce, the SDK and Wings' base styles loaded first. */
  private async pageDocument(plugin: PluginView, viewId: string | null, kind: "page" | "popover" | "settings" | "block", placement: PagePlacement, args?: unknown, focused?: boolean) {
    const { title, file } = this.pageFile(plugin, viewId, kind);
    // A plugin that isn't running, like a turned off one's settings page, has no run to share.
    const run = this.runs.get(plugin.id) ?? newRun();
    const html = await (await fetch(pluginUrl(plugin.id, run, file))).text();
    const base = pluginUrl(plugin.id, run, file.includes("/") ? file.slice(0, file.lastIndexOf("/") + 1) : "");
    const nonce = crypto.randomUUID();
    const head = `<meta charset="utf-8">${nonceScript(nonce, placement, args, focused)}<base href="${base}"><style>${pluginUiCss}</style><script src="${new URL("/plugin-sdk.js", location.href).href}"></script>`;
    const srcdoc = /<head[^>]*>/i.test(html) ? html.replace(/<head[^>]*>/i, (tag) => tag + head) : `<!doctype html><html><head>${head}</head><body>${html}</body></html>`;
    return { title, srcdoc, nonce };
  }

  /**
   * Shows one of a plugin's pages inside `container`, sandboxed like the plugin itself. A sidebar or tab frame lives
   * until the plugin stops or the tab closes, so hiding it keeps its state. `viewId` is null for the settings page.
   * A tab page passes `tab`, which fixes its project and gives it its saved state and args. `hidden` is whether the
   * page mounts out of view. Resolves `remove`, and `setHidden`, which tells the page it is out of view (its `focus`
   * event turns false, so polls pause) or back in view.
   */
  async mountPage(plugin: PluginView, viewId: string | null, container: HTMLElement, kind: "sidebar" | "modal" | "tab" | "popover" | "settings", tab?: PageTab, hidden = false) {
    // A popover belongs to a sidebar button; the settings page lives in Wings Settings, a dialog.
    const placement: PagePlacement = kind === "popover" ? "sidebar" : kind === "settings" ? "modal" : kind;
    const { title, srcdoc, nonce } = await this.pageDocument(plugin, viewId, kind === "popover" || kind === "settings" ? kind : "page", placement, tab?.args, this.focus.focused && !hidden);
    const frame = document.createElement("iframe");
    frame.sandbox.add("allow-scripts", "allow-forms");
    frame.title = title;
    frame.className = "size-full border-0 bg-transparent";
    frame.srcdoc = srcdoc;
    container.replaceChildren(frame);
    this.track(frame, plugin, kind, nonce, viewId ?? undefined, tab);
    const win = frame.contentWindow;
    const running = win ? this.frames.get(win) : undefined;
    if (running) running.hidden = hidden;
    return {
      remove: () => void (win && this.stop(win)),
      setHidden: (next: boolean) => this.setHidden(running, next),
    };
  }

  /** Tells a frame it is out of view or back in view, once per change. */
  private setHidden(running: Running | undefined, hidden: boolean) {
    if (!running || running.hidden === hidden) return;
    running.hidden = hidden;
    if (running.ready) running.post({ event: "focus", data: { focused: this.focus.focused && !hidden } });
  }

  /**
   * Shows one of a plugin's blocks inside `container`, sandboxed like a tab page, fixed to `block.spaceId`'s project.
   * Rejects when that project already has `MAX_LIVE_BLOCKS` block frames. Resolves `remove`, and `setHidden`, which
   * tells the page it is out of view (its `focus` event turns false, so polls pause) or back in view.
   */
  async mountBlock(plugin: PluginView, container: HTMLElement, block: BlockMount) {
    // Checked again after the page loads: blocks that mount together all pass the first check.
    const full = () => {
      if ([...this.frames.values()].filter((r) => r.block?.spaceId === block.spaceId).length >= MAX_LIVE_BLOCKS) {
        throw new Error(`A Project page shows at most ${MAX_LIVE_BLOCKS} blocks at once. Scroll the others out of view or remove one`);
      }
    };
    full();
    const { title, srcdoc, nonce } = await this.pageDocument(plugin, block.blockId, "block", "block", undefined, this.focus.focused && !block.hidden);
    full();
    const frame = document.createElement("iframe");
    frame.sandbox.add("allow-scripts", "allow-forms");
    frame.title = title;
    frame.className = "size-full border-0 bg-transparent";
    frame.srcdoc = srcdoc;
    container.replaceChildren(frame);
    const win = frame.contentWindow;
    if (!win) throw new Error("The block's frame didn't start");
    this.track(frame, plugin, "block", nonce, undefined, undefined, block);
    const running = this.frames.get(win);
    if (running) running.hidden = block.hidden;
    return {
      remove: () => this.stop(win),
      setHidden: (hidden: boolean) => this.setHidden(running, hidden),
    };
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
    const { title, srcdoc, nonce } = await this.pageDocument(target.plugin, target.viewId, "page", "window");
    const { id: pluginId } = target.plugin;
    this.frames.set(
      label,
      this.entry(target.plugin, "window", nonce, target.viewId, {
        post: (message) => this.windows.post(label, { wings: 1, ...message }),
        remove: () => void invoke("plugin_view_close", { pluginId, viewId: target.viewId }),
      }),
    );
    return { srcdoc, title };
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
    this.runs.delete(running.id);
    for (const [token, action] of this.actions) if (action.pluginId === running.id) this.settle(token, new Error("The plugin was turned off"));
    for (const [token, tool] of this.tools) {
      if (tool.pluginId !== running.id) continue;
      this.tools.delete(token);
      void api.mcpToolResult(tool.callId, `${running.id} was turned off in Wings`, true);
    }
    this.callbacks.clearPlugin(running.id);
  }

  /** The project a frame is about: its tab's, or the one on screen. */
  private projectFor(plugin: Running) {
    return this.callbacks.projectOf(plugin.tab?.spaceId ?? plugin.block?.spaceId ?? this.callbacks.currentProject());
  }

  /** Tells the frames that asked for their project when it changed: the one on screen switched, or a branch or name did. */
  publishProjects() {
    for (const plugin of this.frames.values()) {
      if (!plugin.ready || plugin.lastProject === undefined) continue;
      const project = this.projectFor(plugin);
      const json = JSON.stringify(project);
      if (json === plugin.lastProject) continue;
      plugin.lastProject = json;
      plugin.post({ event: "project", data: project });
    }
  }

  /** Tells the frames that asked for the project list when a project was added, removed or renamed, or a branch changed. */
  publishProjectList() {
    let json: string | undefined;
    for (const plugin of this.frames.values()) {
      if (!plugin.ready || plugin.lastProjects === undefined) continue;
      json ??= JSON.stringify(this.callbacks.projects());
      if (json === plugin.lastProjects) continue;
      plugin.lastProjects = json;
      plugin.post({ event: "projects", data: JSON.parse(json) });
    }
  }

  /** A click on a plugin's pane status: shows the page it named (`open`) the way `openView` does, in the page's first place. */
  async openStatusTarget(pluginId: string, viewId: string) {
    const plugin = [...this.frames.values()].find((f) => f.id === pluginId && f.kind === "main");
    const page = plugin && pagesOf(plugin.plugin).find((v) => v.id === viewId);
    if (!plugin || !page) return;
    const place = page.opens[0];
    if (place === "window") await this.openWindow(plugin.plugin, page.id);
    else this.callbacks.showView(pluginId, page.id, place, place === "tab" ? { multiple: page.multiple === true } : undefined);
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

  /** A repo moved: the frames of plugins that may run git hear which projects. */
  private publishGit(projectIds: string[]) {
    for (const plugin of gitListeners(this.frames.values())) plugin.post({ event: "git", data: { projectIds } });
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

  /** Hands an MCP tool call, or a Claude Code hook, from Claude to the plugin's main frame. Rust has checked the plugin declared it. */
  private runTool(call: McpCall) {
    const entry = [...this.frames].find(([, p]) => p.id === call.pluginId && p.kind === "main" && p.ready);
    if (!entry) return void api.mcpToolResult(call.callId, `${call.pluginId} is still starting in Wings. Try again in a moment.`, true);
    const token = crypto.randomUUID();
    this.tools.set(token, { pluginId: call.pluginId, callId: call.callId });
    entry[1].post({ event: "tool", data: { token, name: call.tool, hook: call.hook === true, arguments: call.arguments, paneId: call.paneId, project: call.project, session: call.session } });
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
    void this.unlistenGit.then((off) => off());
    void this.unlistenFocus.then((off) => off());
    this.focus.dispose();
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
      if (msg.method === "ready") {
        plugin.post({ event: "panes", data: this.panes });
        plugin.post({ event: "focus", data: { focused: this.focus.focused && !plugin.hidden } });
      }
    } catch (error) {
      plugin.post({ id, error: String(error instanceof Error ? error.message : error) });
    }
  }

  private async handle(plugin: Running, method: string, p: Record<string, unknown>, output: (line: string) => void) {
    const pluginId = plugin.id;
    // The manager lists what a plugin adds from its manifest, so it can't draw anything it didn't declare.
    const needs = (kind: string) => {
      requireUi(pluginId, plugin.ui, kind);
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
        const args = p.args ?? [];
        if (!Array.isArray(args) || !args.every((a) => typeof a === "string")) throw new Error("openPane args must be a list of strings");
        // Rust checks the command, args and folder; the pane is then opened here, where tabs and splits live.
        const plan = await invoke<Omit<PanePlan, "placement">>("plugin_open_pane", {
          pluginId,
          request: { command: text("command"), args, cwd: text("cwd"), spaceId: this.callbacks.currentProject() },
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
      case "notify": {
        const title = typeof p.title === "string" ? p.title : "";
        const body = typeof p.body === "string" ? p.body : "";
        const tabViews = plugin.plugin.contributes.views.filter((v) => v.opens.includes("tab")).map((v) => v.id);
        const buttons = cleanButtons(p.actions, tabViews);
        const projectId = p.projectId === undefined || p.projectId === null ? null : String(p.projectId);
        if (projectId !== null && !this.callbacks.projectOf(projectId)) throw new Error(`no project ${projectId}`);
        // Rust checks the permission first, so only a notification Wings allowed reaches the list.
        const sent = await invoke("plugin_notify", { pluginId, title, body });
        this.callbacks.notified(plugin.plugin.name, title.slice(0, 200), body.slice(0, 300), { id: pluginId, projectId, buttons });
        return sent;
      }
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
        // Values are what JSON.stringify keeps. Rust checks the sizes; this only stops a value that can't fit
        // from crossing the bridge.
        const text = JSON.stringify(p.value);
        if (text === undefined) throw new Error("wings.storage.set needs a JSON value. Use delete to remove a key");
        if (text.length > 1024 * 1024) throw new Error("A stored value is limited to 1 MB");
        return invoke("plugin_storage_set", { pluginId, key: String(p.key), value: JSON.parse(text) });
      }
      case "storageDelete":
        return invoke("plugin_storage_delete", { pluginId, key: String(p.key) });
      case "storageKeys":
        return invoke("plugin_storage_keys", { pluginId });
      case "storageEntries":
        return invoke("plugin_storage_entries", { pluginId, prefix: String(p.prefix ?? "") });
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
        // Args are JSON stored on the tab, so only a tab has them.
        const hasArgs = p.args !== undefined && p.args !== null;
        if (hasArgs && place !== "tab") throw new Error('openView args only work with as: "tab"');
        const newTab = cleanNewTab(p.newTab, place);
        const args = hasArgs ? JSON.parse(boundedJson(p.args, VIEW_ARGS_MAX, "openView args")) : undefined;
        if (place === "window") await this.openWindow(plugin.plugin, page.id);
        else this.callbacks.showView(pluginId, page.id, place, place === "tab" ? { multiple: page.multiple === true, args, ...(newTab ? { newTab } : {}) } : undefined);
        return null;
      }
      case "navigate":
        // A page in the main window sends this when the user presses a mouse side button over it, since the window
        // never sees a press inside a frame. A view window has no history of its own to move.
        if (plugin.kind === "window") throw new Error("A view window has no back or forward");
        if (p.direction !== "back" && p.direction !== "forward") throw new Error('navigate takes "back" or "forward"');
        this.callbacks.navigate(p.direction);
        return null;
      case "project": {
        const project = this.projectFor(plugin);
        plugin.lastProject = JSON.stringify(project);
        return project;
      }
      case "projects": {
        const projects = this.callbacks.projects();
        plugin.lastProjects = JSON.stringify(projects);
        return projects;
      }
      case "setProjectSection": {
        const sectionId = String(p.sectionId);
        if (!plugin.plugin.contributes.projectSections?.some((s) => s.id === sectionId)) {
          throw new Error(`${pluginId} has no project section ${sectionId}. Declare it in contributes.projectSections`);
        }
        const projectId = String(p.projectId);
        if (!this.callbacks.projectOf(projectId)) throw new Error(`no project ${projectId}`);
        const tabViews = plugin.plugin.contributes.views.filter((v) => v.opens.includes("tab")).map((v) => v.id);
        const rows = p.rows === null ? null : cleanSectionRows(p.rows, tabViews);
        this.callbacks.setProjectSection(pluginId, projectId, sectionId, rows);
        return null;
      }
      case "tabState":
        return plugin.tab ? (this.callbacks.tabState(plugin.tab.tabId) ?? null) : null;
      case "setTabState": {
        if (!plugin.tab) throw new Error("wings.setTabState works in a page open as a tab");
        this.callbacks.setTabState(plugin.tab.tabId, JSON.parse(boundedJson(p.state, TAB_STATE_MAX, "Tab state")));
        return null;
      }
      case "blockState":
        return plugin.block ? (plugin.block.state ?? null) : null;
      case "setBlockState": {
        if (!plugin.block) throw new Error("wings.setBlockState works in a page shown as a block on a Project page");
        const state = JSON.parse(blockStateJson(p.state));
        // The page hears of a failed save, and blockState() only ever gives what was stored.
        await plugin.block.onState(state);
        plugin.block.state = state;
        return null;
      }
      case "setHeight": {
        if (!plugin.block) throw new Error("wings.setHeight works in a page shown as a block on a Project page");
        plugin.block.onHeight(clampBlockHeight(p.px, plugin.block.minHeight));
        return null;
      }
      case "openSettings":
        if (!plugin.plugin.contributes.settings) throw new Error(`${pluginId} has no settings page. Add contributes.settings to its manifest`);
        this.callbacks.openSettings(pluginId);
        // A page in a view window asked, so Settings opens behind it: bring the main window forward. Only then, so a
        // plugin's background script can't pull Wings in front of another app. `setFocus` skips a minimized window.
        if (plugin.kind === "window") {
          const main = getCurrentWebviewWindow();
          await main.unminimize();
          await main.setFocus();
        }
        return null;
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
      case "setPaneStatus": {
        needs("status");
        const paneId = String(p.paneId);
        if (!this.panes.some((s) => s.paneId === paneId)) throw new Error(`no pane ${paneId}`);
        this.callbacks.setPaneStatus(pluginId, paneId, p.status === null ? null : cleanPaneStatus(p.status, pagesOf(plugin.plugin).map((v) => v.id)));
        return null;
      }
      case "setPaneRail": {
        needs("rail");
        const paneId = String(p.paneId);
        if (!this.panes.some((s) => s.paneId === paneId)) throw new Error(`no pane ${paneId}`);
        this.callbacks.setPaneRail(pluginId, paneId, p.rail === null ? null : cleanPaneRail(p.rail, pagesOf(plugin.plugin).map((v) => v.id)));
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
        this.callbacks.openDiff({ id, pluginId, title, subtitle: str(p.subtitle, 200), cwd: str(p.cwd, 1000), ...cleanDiffUpdate(p) });
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
