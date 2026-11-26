// Runs in every frame, after `guard.js`, in Wings' own world (as it finishes loading in WebKit, as it starts in
// Chromium): tells Wings (`wingsGuard`, a handler only that world reaches) once the frame holds a secret field, there as
// it loads or added or turned into one later, in the document or in an open shadow root. Wings keeps that for the rest
// of the main frame's document, so a password a page turns into plain text (Show password) still refuses an agent's
// script.
(() => {
  const tell = () => {
    try {
      webkit.messageHandlers.wingsGuard.postMessage(true);
    } catch (_) {}
  };
  if (wingsScan(document).secret) return tell();
  const field = (n) => n.nodeType === 1 && /^(INPUT|TEXTAREA|SELECT)$/.test(n.tagName) && wingsSecret(n);
  const hit = (n) => field(n) || (n.nodeType === 1 && wingsScan(n).secret);
  const observers = [];
  const watched = new WeakSet();
  let done = false;
  const stop = () => {
    done = true;
    for (const observer of observers) observer.disconnect();
    removeEventListener("focusin", typed, true);
    removeEventListener("input", typed, true);
    tell();
  };
  // A change inside a shadow root is no change of the document's: each open one is watched as the document is, those
  // there now and those that come with what is added.
  const roots = (n) => {
    for (const e of [n, ...n.querySelectorAll("*")]) if (e.shadowRoot) watch(e.shadowRoot);
  };
  const watch = (root) => {
    if (done || watched.has(root)) return;
    watched.add(root);
    const observer = new MutationObserver((records) => {
      if (done) return;
      for (const r of records) {
        if (r.type === "attributes" ? hit(r.target) : Array.from(r.addedNodes).some(hit)) return stop();
        for (const n of r.addedNodes) if (n.nodeType === 1) roots(n);
      }
    });
    observer.observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: ["type", "autocomplete"] });
    observers.push(observer);
    roots(root);
  };
  // A shadow root attached to an element already there makes no change to watch: a field in it is caught as the person
  // focuses it or types in it.
  const typed = (event) => {
    if (!done && event.composedPath().some(field)) stop();
  };
  addEventListener("focusin", typed, true);
  addEventListener("input", typed, true);
  watch(document);
})();
