/// <reference path="./wings.d.ts" />
/** Where a page shows in Wings. Each gets its own padding and spacing from Wings' plugin-ui.css. */
export type Place = "sidebar" | "popover" | "view" | "tab" | "settings";

export interface ShellOptions {
  /** Lay the page out edge to edge with no padding, filling the frame, for a page that scrolls on its own. */
  fill?: boolean;
}

/**
 * Makes the page's `#app` element (or a new `<main>`) the shell for `place`, and returns it, for a page that
 * builds its DOM itself. React pages use `showSidebar` and the others from `wings-plugin-ui` instead.
 */
export function mountShell(place: Place, options?: ShellOptions): HTMLElement;
