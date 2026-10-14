// Wings plugin SDK, API 1. Loaded into each plugin's sandboxed frame before the plugin's main script.
// Plugins talk to Wings only through `window.wings`; every call is checked against the manifest.
(() => {
  // Set by Wings in this document only; every message carries it, so Wings knows it's really this page.
  const nonce = window.__wingsNonce;
  delete window.__wingsNonce;
  // Where this page shows and what it was opened with, set by Wings in the same document as the nonce.
  const init = window.__wingsInit ?? {};
  delete window.__wingsInit;
  let nextId = 1;
  const pending = new Map();
  const listeners = { panes: [], action: [], broadcast: [] };
  let replyHandler = null;
  listeners.transcript = [];
  listeners.focus = [];
  listeners.project = [];
  listeners.projects = [];
  // Whether the user is in Wings. Wings sends it as soon as the frame is ready; until then, true.
  // Wings sets `focused` in the init of a block, which can mount out of view; every other page starts in front.
  let focused = init.focused ?? true;
  let focusKnown = false;
  const tools = new Map();
  // `setHeight`: the height asked for last, the one sent last, and whether a send is queued for the end of this tick.
  let wantedHeight = null;
  let sentHeight = null;
  let heightQueued = false;
  // Each `on*` returns a function that removes the listener, for a page that subscribes in a React effect.
  const subscribe = (list, listener) => {
    list.push(listener);
    return () => {
      const at = list.indexOf(listener);
      if (at >= 0) list.splice(at, 1);
    };
  };

  const call = (method, params, onOutput) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject, onOutput });
      parent.postMessage({ wings: 1, nonce, id, method, params }, "*");
    });

  // The last badge and sidebar label this frame sent, as JSON, so a repeat of what Wings already shows never crosses the bridge.
  // Plugins resend on every poll. Wings clears a plugin's badges and labels only when its main frame stops (a new frame
  // starts with an empty memory) and drops a badge when its pane leaves the list, which `forgetPanes` mirrors.
  const sentBadges = new Map();
  const sentStatuses = new Map();
  const sentLabels = new Map();
  // Sends only when `value` differs from the last one sent under `key`. A call that fails is forgotten, so the next one retries.
  const sendChanged = (sent, key, value, method, params) => {
    const json = JSON.stringify(value);
    if (sent.get(key) === json) return Promise.resolve(null);
    sent.set(key, json);
    return call(method, params).catch((error) => {
      if (sent.get(key) === json) sent.delete(key);
      throw error;
    });
  };
  const forgetPanes = (panes) => {
    const live = new Set(Array.isArray(panes) ? panes.map((p) => p?.paneId) : []);
    for (const paneId of sentBadges.keys()) if (!live.has(paneId)) sentBadges.delete(paneId);
    for (const paneId of sentStatuses.keys()) if (!live.has(paneId)) sentStatuses.delete(paneId);
  };

  // The webview's own menu offers only "Reload", which would reload this frame. Keep it for fields and a selection (Copy).
  addEventListener("contextmenu", (e) => {
    if (e.target instanceof Element && e.target.closest("input, textarea, [contenteditable]")) return;
    if (getSelection()?.toString()) return;
    e.preventDefault();
  });

  // A mouse's back and forward buttons (3 and 4) pressed over this page: the window around it never sees them, so ask
  // it to go back or forward. Only a real press counts: `isTrusted` is false for a script's `dispatchEvent`. The
  // press is swallowed like the window does, so the page doesn't also treat it as a click.
  const sideButton = (e) => {
    if (!e.isTrusted || (e.button !== 3 && e.button !== 4)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.type === "mouseup") call("navigate", { direction: e.button === 3 ? "back" : "forward" }).catch(() => {});
  };
  for (const type of ["mousedown", "mouseup", "auxclick"]) addEventListener(type, sideButton, true);

  addEventListener("message", (event) => {
    if (event.source !== parent || event.data?.wings !== 1) return;
    const message = event.data;
    if (message.output) return void pending.get(message.output.call)?.onOutput?.(message.output.line);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) reject(new Error(message.error));
      else resolve(message.result);
      return;
    }
    if (message.event === "panes") forgetPanes(message.data);
    if (message.event === "focus") {
      if (message.data?.focused === focused && focusKnown) return;
      focused = message.data?.focused === true;
      focusKnown = true;
    }
    if (message.event === "action") return void runAction(message.data);
    if (message.event === "tool") return void runTool(message.data);
    if (message.event === "reply") return void runReply(message.data);
    for (const listener of [...(listeners[message.event] ?? [])]) {
      try {
        listener(message.data);
      } catch (error) {
        console.error(error);
      }
    }
  });

  // Wings shows a spinner on the clicked button until every action listener has settled.
  async function runAction({ token, ...data }) {
    let error = null;
    for (const listener of [...listeners.action]) {
      try {
        await listener(data);
      } catch (e) {
        error = String(e?.message ?? e);
        console.error(e);
      }
    }
    call("actionDone", { token, error });
  }

  // The viewer keeps the reply box busy until this settles, and shows the error if it throws.
  async function runReply({ token, ...data }) {
    let error = null;
    try {
      if (!replyHandler) throw new Error("This plugin doesn't take replies");
      await replyHandler(data);
    } catch (e) {
      error = String(e?.message ?? e);
    }
    call("actionDone", { token, error });
  }

  // Claude called one of the plugin's MCP tools. A thrown error goes back to Claude as a failed call.
  async function runTool({ token, name, arguments: input, paneId }) {
    let text;
    let isError = false;
    try {
      const handler = tools.get(name);
      if (!handler) throw new Error(`The plugin has no handler for ${name}`);
      const result = await handler(input ?? {}, { paneId: paneId ?? null });
      text = typeof result === "string" ? result : result === undefined ? "Done" : JSON.stringify(result, null, 2);
    } catch (e) {
      text = String(e?.message ?? e);
      isError = true;
    }
    call("toolDone", { token, text, isError });
  }

  window.wings = Object.freeze({
    /** Every terminal pane `{ paneId, cwd, command, project, session, focused }`; `session` is set while Claude runs, `focused` on the focused pane of the tab on screen. Called on every change. */
    onPanes: (listener) => subscribe(listeners.panes, listener),
    /**
     * Whether the user is in Wings: `{ focused }`, true while any Wings window (the main one, or a view window) is in
     * front. Called once with the current state, then on each change. Pause timers while it is false and check
     * once when it turns true. Needs no permission.
     */
    onFocus: (listener) => {
      const off = subscribe(listeners.focus, listener);
      // Registered after Wings sent the state: tell this listener where things stand.
      if (focusKnown) queueMicrotask(() => { try { listener({ focused }); } catch (error) { console.error(error); } });
      return off;
    },
    /** True while any Wings window is in front. Changes are announced to `onFocus`. */
    get focused() {
      return focused;
    },
    /** A badge action was clicked: `{ paneId, actionId }`. */
    onAction: (listener) => subscribe(listeners.action, listener),
    /**
     * Handles calls to one of the manifest's `contributes.mcpTools`, from the main script. `handler(input, { paneId })`
     * gets the arguments Claude sent and returns a string or a JSON value, or a promise of one. `paneId` is the
     * Wings pane Claude runs in, or `null`.
     */
    onTool: (name, handler) => void tools.set(name, handler),
    /**
     * Runs a program from the manifest's `permissions.exec`. Resolves `{ code, stdout, stderr }`.
     * `timeoutMs` defaults to 30 s, max 5 min. `onOutput(line)` gets each stdout or stderr line while it runs.
     */
    exec: (program, args = [], { cwd = null, timeoutMs = null, onOutput } = {}) =>
      call("exec", { program, args, cwd, timeoutMs, stream: typeof onOutput === "function" }, onOutput),
    /** Transcript entries of the given types (must be in `permissions.transcript`), oldest first. `last` keeps only the newest that many, up to 1000. */
    transcript: (sessionId, types, { last } = {}) => call("transcript", { sessionId, types, last }),
    /**
     * Each transcript entry Claude Code writes while it runs in a pane, as `{ sessionId, paneId, entry }`, for the
     * entry types in `permissions.transcript`. Only entries written after the plugin started, or after the session
     * started in the pane; `transcript` reads earlier ones. Entries over 256 KB are skipped.
     */
    onTranscript: (listener) => subscribe(listeners.transcript, listener),
    /**
     * What Claude Code last told its status line, with `permissions.statusline`, once `wings statusline` is your
     * `statusLine` command. Resolves `{ rateLimits, sessions }`: the 5-hour and weekly limits, or `null` until a
     * session reports them, and each session's context window, token counts and prompt cache by session id.
     */
    statusline: () => call("statusline", {}),
    /**
     * With `permissions.ports`: the local TCP ports the programs in your panes listen on, as
     * `{ paneId, pid, name, port, startedAt }[]`. Only ports `localhost` reaches. Costs about 20 ms: poll slowly.
     */
    ports: () => call("ports", {}),
    /** Shows a badge in a pane's header, or removes it with `null`. Sending what this frame last sent for the pane does nothing. */
    setBadge: (paneId, badge) => sendChanged(sentBadges, paneId, badge, "setBadge", { paneId, badge }),
    /**
     * Shows a slim status strip under a pane's terminal, or removes it with `null`. Needs `"status"` in `contributes.ui`.
     * `{ items, rows?, open? }`: up to 4 `items` of `{ label?, value, percent?, tone? }` (label up to 16 characters, value up to 32,
     * `percent` 0 to 100 draws a small meter), `rows` of `{ label, value, tone? }` for the card shown on hover, and `open`, the id of
     * one of the plugin's sidebars or views, which a click opens. One status per plugin per pane. Rejects a status that breaks a limit.
     * Sending what this frame last sent for the pane does nothing.
     */
    setPaneStatus: (paneId, status) => sendChanged(sentStatuses, paneId, status, "setPaneStatus", { paneId, status }),
    /** Opens an https URL, or an http one to localhost, 127.0.0.1 or [::1], matching `permissions.openUrl` in the browser. */
    openUrl: (url) => call("openUrl", { url }),
    /**
     * Opens a terminal in a new tab, or in a split beside the focused pane with `placement` "right" or "down".
     * Resolves `{ paneId }` once the pane is in `onPanes`. `command` is typed into its shell and must be covered
     * by `permissions.panes`. `cwd` must be inside one of your projects; without it the pane opens at the root
     * of the project on screen.
     */
    openPane: ({ command = null, cwd = null, placement = "tab" } = {}) => call("openPane", { command, cwd, placement }),
    /** Shows an open pane and moves the keyboard to it. Needs `permissions.panes`. */
    focusPane: (paneId) => call("focusPane", { paneId }),
    /**
     * Asks Wings to install a command-line tool the plugin runs, like "gh", when `exec` rejected with
     * "<program> is not installed or not on PATH". Wings shows its own confirmation and installs it the vendor's
     * way in a new tab, then restarts the plugin once it's there. Resolves true if you started the install (or
     * it's already installed), false if you cancelled. Only az, gh and glab, and only ones in `permissions.exec`.
     */
    installTool: (program) => call("installTool", { program }),
    /** A desktop notification, with `permissions.notify`. Up to 20 a minute; there's no click action. */
    notify: ({ title, body = "" } = {}) => call("notify", { title, body }),
    /**
     * An HTTP request to a URL under `permissions.fetch`, made by Wings. An entry may be `GET <prefix>`, or
     * `POST <url with * segments> [{field=value}]` for one write with a JSON `body`. `bearer` names a secret that Wings
     * sends as `Authorization: Bearer <secret>`. Resolves `{ status, contentType, body }` for any status;
     * redirects aren't followed.
     */
    fetch: (url, { method, headers, body, bearer } = {}) => call("fetch", { url, method, headers, body, bearer }),
    /** Secrets, like API tokens, kept in the system keychain. They can be used by `fetch` but not read back. */
    secrets: Object.freeze({
      set: (name, value) => call("secretSet", { name, value }),
      delete: (name) => call("secretDelete", { name }),
      has: (name) => call("secretHas", { name }),
    }),
    /**
     * The plugin's own key-value storage for settings and state, since `localStorage` throws in plugin frames.
     * Values are JSON. Keys are 1 to 128 letters, digits and `- _ . : /`, and everything together is limited to
     * 1 MB. Only this plugin's pages see it, and it goes when the plugin is removed.
     */
    storage: Object.freeze({
      /** Resolves the value, or `null` when the key isn't set. */
      get: (key) => call("storageGet", { key }),
      set: (key, value) => call("storageSet", { key, value }),
      delete: (key) => call("storageDelete", { key }),
      /** Resolves every key, sorted. */
      keys: () => call("storageKeys", {}),
    }),
    /**
     * Short text next to a sidebar's or view's title bar button, like a running timer. `null` clears it. `rows`, up to 6
     * `{ label, value }`, show when you point at the button.
     */
    setSidebarLabel: (sidebarId, label, { rows, tone } = {}) =>
      sendChanged(sentLabels, sidebarId, [label, rows, tone], "setSidebarLabel", { sidebarId, label, rows, tone }),
    /**
     * Opens one of the manifest's sidebars or views: in the sidebar, in a modal over Wings, in a window of its own
     * or as a tab in the project on screen, with `as` one of the view's `opens` (the first by default). A window or
     * tab that's already open comes to the front. The page gets the same `wings` object wherever it opens.
     *
     * `args` (JSON, up to 16 KB, only with `as: "tab"`) is what the tab's page reads with `viewArgs`. A tab that's
     * already open with other args takes the new ones and reloads; a view with `multiple` opens a new tab unless one in that project already has the same args.
     */
    openView: (id, { as, args } = {}) => call("openView", { id, as, args }),
    /** Where this page shows: "sidebar", "tab", "modal", "window" or "block". `null` in the plugin's main script. */
    placement: () => init.placement ?? null,
    /** What `openView` passed as `args` when it opened this tab, or `null`. Available at once, as a copy. */
    viewArgs: () => (init.args == null ? null : structuredClone(init.args)),
    /**
     * The project this page is about, `{ id, name, path, branch }`, or `null` when none is open. A tab page's project is
     * the one the tab belongs to and never changes; any other page gets the project on screen.
     */
    project: () => call("project", {}),
    /**
     * Calls `listener` with the current project, then each time it changes: the project on screen switched (not for a
     * tab page), or its branch or name changed. Returns a function that removes the listener.
     */
    onProject: (listener) => {
      const off = subscribe(listeners.project, listener);
      let live = true;
      call("project", {}).then(
        (project) => {
          if (live) listener(project);
        },
        (error) => console.error(error),
      );
      return () => {
        live = false;
        off();
      };
    },
    /** Every project in Wings, `{ id, name, path, branch }[]` in sidebar order. Worktrees are projects too. */
    projects: () => call("projects", {}),
    /**
     * Calls `listener` with every project, then each time one is added, removed or renamed, or a branch changes.
     * Returns a function that removes the listener.
     */
    onProjects: (listener) => {
      const off = subscribe(listeners.projects, listener);
      let live = true;
      call("projects", {}).then(
        (projects) => {
          if (live) listener(projects);
        },
        (error) => console.error(error),
      );
      return () => {
        live = false;
        off();
      };
    },
    /**
     * Shows a list under a project in the sidebar, in the section `sectionId` of the manifest's `contributes.projectSections`,
     * or removes it with `rows: null`. A row is `{ id, label, detail?, badge?: { text, tone? }, open: { view, args? } }`:
     * clicking it opens `open.view`, one of your views that opens as a tab, in that project, with `args`. Up to 50 rows;
     * Wings cuts text to 120 characters (badge 24) and rejects args over 16 KB. Rejects for a project that isn't there, a
     * section you didn't declare or a view that doesn't open as a tab. Sections are kept in memory and go when the plugin
     * stops, so fill them again on start.
     */
    setProjectSection: (projectId, sectionId, rows) => call("setProjectSection", { projectId, sectionId, rows }),
    /** In a tab page: the state it last saved with `setTabState`, or `null`. Outside a tab it is `null`. */
    tabState: () => call("tabState", {}),
    /**
     * In a tab page: saves JSON state (up to 64 KB) on the tab. Wings keeps it with the tab across restarts, and across
     * the page being unloaded while hidden. Rejects outside a tab or when the state is too big.
     */
    setTabState: (state) => call("setTabState", { state }),
    /** In a block on a Project page: the state it last saved with `setBlockState`, or `null`. Outside a block it is `null`. */
    blockState: () => (init.placement === "block" ? call("blockState", {}) : Promise.resolve(null)),
    /**
     * In a block: saves JSON state (up to 16 KB) on the block. Wings keeps it with the Project page across restarts, and
     * across the block being unloaded while out of view. Rejects outside a block or when the state is too big.
     */
    setBlockState: (state) => (init.placement === "block" ? call("setBlockState", { state }) : Promise.reject(new Error("wings.setBlockState works in a page shown as a block on a Project page"))),
    /**
     * In a block: asks for a height in CSS pixels, which Wings clamps to the block's `minHeight` and 1200. Call it when
     * the content's height changes; asking for the height already asked for does nothing, and several calls in one tick send the last.
     * Throws outside a block.
     */
    setHeight: (px) => {
      if (init.placement !== "block") throw new Error("wings.setHeight works in a page shown as a block on a Project page");
      if (typeof px !== "number" || !Number.isFinite(px)) throw new TypeError("wings.setHeight needs a number of pixels");
      const height = Math.round(px);
      if (height === wantedHeight) return;
      wantedHeight = height;
      if (heightQueued) return;
      heightQueued = true;
      queueMicrotask(() => {
        heightQueued = false;
        if (wantedHeight === sentHeight) return;
        sentHeight = wantedHeight;
        call("setHeight", { px: sentHeight }).catch((error) => console.error(error));
      });
    },
    /** Closes a sidebar or view wherever it's open. */
    closeView: (id) => call("closeView", { id }),
    /** Opens Wings Settings at this plugin's settings page, the manifest's `contributes.settings`. */
    openSettings: () => call("openSettings", {}),
    /** Sends a JSON value to the plugin's other pages (its main script and open sidebars, modals and windows), up to 64 KB. */
    broadcast: (message) => call("broadcast", { message }),
    /** Gets what the plugin's other pages send with `broadcast`. */
    onBroadcast: (listener) => subscribe(listeners.broadcast, listener),
    /**
     * Opens the Wings diff viewer. Leave out `patch` to open it in a loading state, then fill it with
     * `updateDiff(id, { patch, comments })` or `updateDiff(id, { error })`. Resolves `{ id }`. `focus`,
     * `{ path, line?, side?: "old" | "new" }`, makes the viewer open that file once the patch is shown, scroll to
     * the line and highlight it for a moment. It works when the patch arrives later through `updateDiff`, and a
     * path that isn't in the patch just opens the diff as usual. `updateDiff` takes `focus` too.
     */
    openDiff: (diff) => call("openDiff", diff),
    /**
     * A reply typed under a comment with `canReply` in the diff viewer: `{ diffId, replyTo, body }`, where
     * `replyTo` is the thread's first comment. Post it, then add it with `updateDiff`. A thrown error shows
     * under the reply box and keeps the text.
     */
    onReply: (handler) => void (replyHandler = handler),
    /**
     * POSTs `fields` as JSON to an API path in `permissions.post`, through that CLI's own sign-in: `gh api`,
     * `glab api` (pass the GitLab `host`; for `gh` pass a GitHub Enterprise Server's) or `az rest` for Azure
     * DevOps. Field values are strings or whole numbers; a manifest entry may pin a field, as in
     * `gh <path> {event=APPROVE}`. Resolves `{ code, stdout, stderr }`.
     */
    post: (program, url, fields, { host } = {}) => call("post", { program, url, fields, host }),
    /** Updates a viewer opened with `openDiff`; ignored once the user has closed it. */
    updateDiff: (id, update) => call("updateDiff", { id, ...update }),
  });

  call("ready", {});
})();
