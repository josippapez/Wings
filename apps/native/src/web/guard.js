// The secret fields of a page: a password, a card's number, name, date or code, or a one-time code, as the Browser's
// agent helper names them (`secret` in plugins/browser/rust/src/agent_js.rs). Run by Wings itself in a world of its own
// (`wings-guard`), which the page's scripts and the plugin's can't reach: as a check right before an agent's script or a
// picture (`guard::CHECK`), and in every frame as it loads and while it changes (`watch.js`), in WebKit and Chromium.

function wingsSecret(e) {
  return e.type === "password" || /(^|\s)(cc-|one-time-code|current-password|new-password)/.test(e.getAttribute("autocomplete") || "");
}

// Whether `root` (a document, an element or a shadow root) holds one, filled or not, and where each shows in the
// viewport, in CSS pixels: over the frames of its origin a script of it can reach, and their open shadow roots. A frame
// of another origin can't be looked into: it is listed apart (`foreign`), to paint over whole when one of those frames
// said it holds one. Past the deepest it looks, a shadow root or frame counts as holding one, painted whole.
function wingsScan(root) {
  let found = false;
  const rects = [];
  const foreign = [];
  const box = (r, dx, dy) => [r.left + dx, r.top + dy, r.width, r.height];
  const scan = (root, dx, dy, depth) => {
    for (const e of root.querySelectorAll("input, textarea, select")) {
      if (!wingsSecret(e)) continue;
      found = true;
      for (const r of e.getClientRects()) {
        if (r.width > 0 && r.height > 0) rects.push(box(r, dx, dy));
      }
    }
    for (const e of root.querySelectorAll("*")) {
      const frame = e.tagName === "IFRAME" || e.tagName === "FRAME";
      if (!frame && !e.shadowRoot) continue;
      const r = e.getBoundingClientRect();
      if (depth >= 8) {
        found = true;
        rects.push(box(r, dx, dy));
        continue;
      }
      if (e.shadowRoot) scan(e.shadowRoot, dx, dy, depth + 1);
      if (!frame) continue;
      let inner = null;
      try {
        inner = e.contentDocument;
      } catch (_) {}
      if (inner) scan(inner, dx + r.left + e.clientLeft, dy + r.top + e.clientTop, depth + 1);
      else foreign.push(box(r, dx, dy));
    }
  };
  scan(root, 0, 0, 0);
  // An element's own shadow root is none of its descendants': one added with a field already in it shows only here.
  if (root.shadowRoot) scan(root.shadowRoot, 0, 0, 1);
  return { secret: found, rects, foreign };
}
