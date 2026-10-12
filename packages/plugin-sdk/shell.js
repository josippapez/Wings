// The frame a plugin page lives in, without React: where it is, its padding and how it scrolls come from Wings'
// plugin-ui.css, so a page only says which place it is. wings-plugin-ui mounts React into it.
const PLACES = ["sidebar", "popover", "view", "tab", "settings"];
// Sidebars and popovers change while you look at them, so a screen reader hears what changed.
const LIVE = new Set(["sidebar", "popover"]);

/**
 * Makes the page's `#app` element (or a new `<main>`) the shell for `place`, and returns it. Calling it again
 * returns the same element. `fill` lays the page out edge to edge, for a page that scrolls on its own.
 */
export function mountShell(place, { fill = false } = {}) {
  if (!PLACES.includes(place)) throw new Error(`Unknown place ${place}: use one of ${PLACES.join(", ")}`);
  let shell = document.getElementById("app");
  if (!shell) {
    shell = document.createElement("main");
    shell.id = "app";
    document.body.append(shell);
  }
  shell.classList.add("w-shell");
  shell.dataset.place = place;
  shell.toggleAttribute("data-fill", fill);
  if (LIVE.has(place)) shell.setAttribute("aria-live", "polite");
  else shell.removeAttribute("aria-live");
  return shell;
}
