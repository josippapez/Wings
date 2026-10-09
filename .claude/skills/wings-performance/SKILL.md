---
name: wings-performance
description: Keeps the Wings desktop app smooth. Use before adding or changing anything that runs per frame, per resize, per keystroke or on a timer, any animation or drag that changes the size of the pane area, and any polling, in apps/desktop or a plugin. Also use when the user reports jank, lag, dropped frames or terminals glitching.
---

# Performance in Wings

Wings is a GUI around live terminals, and terminals are the expensive part. Most slowdowns come from doing terminal work, React renders or network calls far more often than the screen needs.

## Rules

1. **Nothing heavy per frame.** A sidebar slide or a handle drag changes the pane area's size on every frame. Work that reacts to size waits until the size holds still. `TerminalSession` in `apps/desktop/src/lib/terminal.ts` refits 80 ms after the last `ResizeObserver` callback, because each refit reflows up to 10,000 lines of scrollback and resizes the PTY, which makes the shell or Claude redraw. Never call `fit()` or `paneResize` from an animation frame, a drag handler or an undebounced observer.
2. **No App renders per frame.** `App.tsx` renders every pane. Commit drag results to state once the gesture ends: the sidebar widths come from the panel group's `onLayoutChanged`, and only when `meta.isUserInteraction` is true. A panel's `onResize` also fires with the in-between sizes of a CSS slide, which once saved a 1px sidebar.
3. **Animate with CSS, not JS-driven layout.** Sidebars slide with a CSS `flex-grow` transition that `App.tsx` turns on only while opening or closing (`[data-sliding]` in `apps/desktop/src/index.css`). Respect `prefers-reduced-motion`.
4. **Hidden panes cost nothing.** Only visible panes hold a WebGL context. WebKit allows 16 per page and silently drops the oldest.
5. **Poll slowly, and only while it matters.** Local checks every 15 s, network every 60 s, and only while Wings is focused, like `start_git_status` in `apps/desktop/src-tauri/src/lib.rs`. Offer a Refresh button rather than polling faster. Cache network lookups per repo and branch, not per pane.
6. **A click never waits on the network.** Open the view in a loading state, then fill it. Prefetch heavy data after the cheap status loads.

## Measure before and after

State a number for the change, not "feels faster".

- **Layout and resize work in a browser.** Run `pnpm exec vite --port 1420` in `apps/desktop`, then open it with the Tauri stub from this skill, which answers every command and records it in `window.__calls`:

  ```sh
  agent-browser --init-script .claude/skills/wings-performance/tauri-stub.js open http://localhost:1420
  ```

  Count the expensive calls during the gesture, for example `window.__calls.filter(c => c.cmd === "pane_resize").length` while a sidebar opens. It should be 1, not one per frame. Sample sizes with `requestAnimationFrame` to check an animation runs and how long it takes. `agent-browser mouse down`, `move` and `up` drive a drag.
- **Terminal rendering.** `WINGS_BENCH=1` runs `apps/desktop/src/bench.ts` inside the real webview and prints a report.
- **Renderer comparison and memory.** `apps/desktop/scripts/bench-renderers.sh` replays a recorded Claude Code session into many panes per renderer and reports footprints, graphics memory, CPU and frame times (see `apps/desktop/README.md`). In Chromium, load `bench-stub.js` after `tauri-stub.js` and open `/?renderer=dom|webgl|canvas|canvas-shared`.
- **Plugin exec calls.** The dev log prints `[plugin] <id> <program> took N ms` for every call.

The stub runs in Chromium. CSS transitions and WebGL behave differently in WKWebView, so check what the user will see in the built app too. Don't click through the GUI while the user is using their Mac. Capture the window instead.
