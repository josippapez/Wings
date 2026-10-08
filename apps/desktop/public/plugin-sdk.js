// Wings plugin SDK, API 1. Loaded into each plugin's sandboxed frame before the plugin's main script.
// Plugins talk to Wings only through `window.wings`; every call is checked against the manifest.
(() => {
  let nextId = 1;
  const pending = new Map();
  const listeners = { panes: [], action: [] };

  const call = (method, params, onOutput) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject, onOutput });
      parent.postMessage({ wings: 1, id, method, params }, "*");
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

  window.wings = Object.freeze({
    /** Every terminal pane `{ paneId, cwd, command, project, session }`; `session` is set while Claude runs. Called on every change. */
    onPanes: (listener) => void listeners.panes.push(listener),
    /** A badge action was clicked: `{ paneId, actionId }`. */
    onAction: (listener) => void listeners.action.push(listener),
    /**
     * Runs a program from the manifest's `permissions.exec`. Resolves `{ code, stdout, stderr }`.
     * `timeoutMs` defaults to 30 s, max 5 min. `onOutput(line)` gets each stdout or stderr line while it runs.
     */
    exec: (program, args = [], { cwd = null, timeoutMs = null, onOutput } = {}) =>
      call("exec", { program, args, cwd, timeoutMs, stream: typeof onOutput === "function" }, onOutput),
    /** Transcript entries of the given types (must be in `permissions.transcript`). */
    transcript: (sessionId, types) => call("transcript", { sessionId, types }),
    /** Shows a badge in a pane's header, or removes it with `null`. */
    setBadge: (paneId, badge) => call("setBadge", { paneId, badge }),
    /** Opens an https URL matching `permissions.openUrl` in the browser. */
    openUrl: (url) => call("openUrl", { url }),
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
