// Wings plugin SDK, API 1. Loaded into each plugin's sandboxed frame before the plugin's main script.
// Plugins talk to Wings only through `window.wings`; every call is checked against the manifest.
(() => {
  // Set by Wings in this document only; every message carries it, so Wings knows it's really this page.
  const nonce = window.__wingsNonce;
  delete window.__wingsNonce;
  let nextId = 1;
  const pending = new Map();
  const listeners = { panes: [], action: [], broadcast: [] };
  listeners.transcript = [];
  const tools = new Map();

  const call = (method, params, onOutput) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject, onOutput });
      parent.postMessage({ wings: 1, nonce, id, method, params }, "*");
    });

  // The webview's own menu offers only "Reload", which would reload this frame. Keep it for fields and a selection (Copy).
  addEventListener("contextmenu", (e) => {
    if (e.target instanceof Element && e.target.closest("input, textarea, [contenteditable]")) return;
    if (getSelection()?.toString()) return;
    e.preventDefault();
  });

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
    if (message.event === "action") return void runAction(message.data);
    if (message.event === "tool") return void runTool(message.data);
    for (const listener of listeners[message.event] ?? []) {
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
    for (const listener of listeners.action) {
      try {
        await listener(data);
      } catch (e) {
        error = String(e?.message ?? e);
        console.error(e);
      }
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
    /** Every terminal pane `{ paneId, cwd, command, project, session }`; `session` is set while Claude runs. Called on every change. */
    onPanes: (listener) => void listeners.panes.push(listener),
    /** A badge action was clicked: `{ paneId, actionId }`. */
    onAction: (listener) => void listeners.action.push(listener),
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
    onTranscript: (listener) => void listeners.transcript.push(listener),
    /** Shows a badge in a pane's header, or removes it with `null`. */
    setBadge: (paneId, badge) => call("setBadge", { paneId, badge }),
    /** Opens an https URL matching `permissions.openUrl` in the browser. */
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
    /** A desktop notification, with `permissions.notify`. Up to 3 a minute; there's no click action. */
    notify: ({ title, body = "" } = {}) => call("notify", { title, body }),
    /**
     * An HTTP request to a URL under `permissions.fetch`, made by Wings. `bearer` names a secret that Wings
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
     * Short text next to a sidebar's title bar button, like a running timer. `null` clears it. `rows`, up to 6
     * `{ label, value }`, show when you point at the button.
     */
    setSidebarLabel: (sidebarId, label, { rows } = {}) => call("setSidebarLabel", { sidebarId, label, rows }),
    /** Sends a JSON value to the plugin's other pages (its main script and open sidebars), up to 64 KB. */
    broadcast: (message) => call("broadcast", { message }),
    /** Gets what the plugin's other pages send with `broadcast`. */
    onBroadcast: (listener) => void listeners.broadcast.push(listener),
    /**
     * Opens the Wings diff viewer. Leave out `patch` to open it in a loading state, then fill it with
     * `updateDiff(id, { patch, comments })` or `updateDiff(id, { error })`. Resolves `{ id }`.
     */
    openDiff: (diff) => call("openDiff", diff),
    /** Updates a viewer opened with `openDiff`; ignored once the user has closed it. */
    updateDiff: (id, update) => call("updateDiff", { id, ...update }),
  });

  call("ready", {});
})();
