// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// Shows the dev server the pane badge sent here. The address comes from the plugin's storage (when the window
// opens) or a broadcast (when it's already open), and only a loopback http address is ever loaded.

const site = /** @type {HTMLIFrameElement} */ (document.getElementById("site"));
const empty = /** @type {HTMLElement} */ (document.getElementById("empty"));
const label = /** @type {HTMLElement} */ (document.getElementById("address"));
const reload = /** @type {HTMLButtonElement} */ (document.getElementById("reload"));

const LOOPBACK = /^http:\/\/(localhost|127\.0\.0\.1):\d{1,5}\//;

/** @param {unknown} url */
function show(url) {
  if (typeof url !== "string" || !LOOPBACK.test(url)) return;
  label.textContent = url;
  empty.hidden = true;
  site.hidden = false;
  reload.hidden = false;
  site.src = url;
}

reload.addEventListener("click", () => {
  const url = site.src;
  site.src = "about:blank";
  site.src = url;
});
wings.onBroadcast((message) => show(/** @type {any} */ (message)?.preview));
void wings.storage.get("preview").then(show);
