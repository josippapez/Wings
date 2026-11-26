// Runs in every frame as it finishes loading, after `guard.js`, in Wings' own world: tells Wings (`wingsGuard`, a
// handler only that world reaches) once the frame holds a secret field, there as it loads or added or turned into one
// later. Wings keeps that for the rest of the main frame's document, so a password a page turns into plain text (Show
// password) still refuses an agent's script.
(() => {
  const tell = () => {
    try {
      webkit.messageHandlers.wingsGuard.postMessage(true);
    } catch (_) {}
  };
  if (wingsScan(document).secret) return tell();
  const hit = (n) => n.nodeType === 1 && ((/^(INPUT|TEXTAREA|SELECT)$/.test(n.tagName) && wingsSecret(n)) || wingsScan(n).secret);
  new MutationObserver((records, observer) => {
    if (records.some((r) => (r.type === "attributes" ? hit(r.target) : Array.from(r.addedNodes).some(hit)))) {
      observer.disconnect();
      tell();
    }
  }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ["type", "autocomplete"] });
})();
