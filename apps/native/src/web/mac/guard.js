// The secret fields of the page: a password, a card's number, name, date or code, or a one-time code, as the Browser's
// agent helper names them (`secret` in plugins/browser/rust/src/agent_js.rs). Run by Wings itself in a world of its own
// (`wings-guard`), which the page's scripts and the plugin's can't reach, over the main frame and every frame of the same
// origin a script of the page could reach, and their open shadow roots. Answers whether there is any, filled or not, and
// where each shows in the main frame's viewport, in CSS pixels, to paint over in a picture.
const secret = (e) =>
  e.type === "password" || /(^|\s)(cc-|one-time-code|current-password|new-password)/.test(e.getAttribute("autocomplete") || "");
let found = false;
const rects = [];
const scan = (root, dx, dy, depth) => {
  if (depth > 8) return;
  for (const e of root.querySelectorAll("input, textarea, select")) {
    if (!secret(e)) continue;
    found = true;
    for (const r of e.getClientRects()) {
      if (r.width > 0 && r.height > 0) rects.push([r.left + dx, r.top + dy, r.width, r.height]);
    }
  }
  for (const e of root.querySelectorAll("*")) {
    if (e.shadowRoot) scan(e.shadowRoot, dx, dy, depth + 1);
    if (e.tagName === "IFRAME" || e.tagName === "FRAME") {
      let inner = null;
      try {
        inner = e.contentDocument;
      } catch (_) {}
      if (!inner) continue;
      const r = e.getBoundingClientRect();
      scan(inner, dx + r.left + e.clientLeft, dy + r.top + e.clientTop, depth + 1);
    }
  }
};
scan(document, 0, 0, 0);
return { secret: found, rects };
