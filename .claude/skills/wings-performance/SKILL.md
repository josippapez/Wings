---
name: wings-performance
description: Keeps the Wings native app (apps/native, GPUI) smooth. Use before adding or changing anything that runs per frame, per resize, per keystroke or on a timer, any animation or drag that changes the size of the pane area, and any polling, in apps/native, a crate it uses, or a plugin. Also use when the user reports jank, lag, dropped frames, high CPU or memory, or terminals glitching.
---

# Performance in Wings

Wings is a native app drawn with GPUI, around live terminals, and the terminals are the expensive part. Most slowdowns come from resizing terminals, redrawing or calling the network far more often than the screen needs.

## Rules

1. **Nothing heavy per frame.** A sidebar slide or an edge drag changes the pane area's size on every frame. Work that reacts to size waits until the size holds still. `TerminalView::layout` in `apps/native/src/terminal_view/mod.rs` sends the grid and PTY resize 80 ms (`RESIZE_DELAY`) after the bounds stop changing, so a slide resizes the shell once. Never resize a terminal from a drag handler or an animation frame. Let `layout()` pick up the new size.
2. **Commit a drag once it ends.** An edge drag (left sidebar, right sidebar, docked diff) saves nothing until the pointer is released, and then writes the file once (`workspace/sidebar/mod.rs`).
3. **Animate with `motion`, and respect Reduce motion.** Use `motion::tween` and the durations in `apps/native/src/ui/motion.rs`. A tween asks for frames only while it runs. `motion::reduced(cx)` decides what a transition keeps: the sidebar drops its slide and keeps its fade. Spinners share one 30-a-second clock (`ui/status_dot.rs`), so any number of them cost one redraw per step. In a page, spinners, working dots and shimmering text move in the window's layer (`ui::view_spinner_layer`, `ui/shimmer.rs`), so a step redraws the layer and never the page: a page that redrew for its spinners rebuilt the whole chat 30 times a second, about a full core while Claude worked. Never put a repeating `with_animation` in a page. Animated backgrounds draw in a Core Animation layer under the window (`ui/native_backdrop.rs`), so a step draws no window frame.
4. **Hidden things cost nothing.** A hidden pane keeps reading its shell but stops painting. After 10 minutes hidden (`UNLOAD_AFTER`) its scrollback becomes text: 10,000 lines cost about 51 MB as cells and 0.5 MB as text (measured). A hidden window stops making screens. Images that leave the screen drop their textures (`window.drop_image`). Put parts that rarely change in a `.cached(...)` piece (`workspace/parts.rs`), so a terminal's output doesn't rebuild the sidebar.
5. **Poll slowly, and only while it matters.** Local checks every 15 s or slower, network every 60 s or slower, and only while Wings is in front (`window_active`), like the git sweep in `workspace/sidebar/git.rs` and the usage poll in `crates/wings-connect/src/usage.rs`. Prefer an event to a poll, as the git sweep follows `wings-gitwatch`. After a 429 or a server error, back off, and wait for `Retry-After` when the answer has one. A plugin polls with `poll::while_focused` from the Rust plugin SDK (`packages/plugin-rs/src/poll.rs`). Offer a Refresh button rather than polling faster. Cache network lookups per repo and branch, not per pane.
6. **A click never waits.** Open the view in a loading state, then fill it. Process scans, keychain reads, HTTP and git run on `cx.background_executor()`, never on the UI thread.

## Measure before and after

State a number for the change, not "feels faster". Run from the repo root. The examples need `--features test-support` and draw offscreen with real Metal, so they run on macOS only. Use `--release` for timings.

- **Frame cost while an agent works:** `cargo run --release --manifest-path apps/native/Cargo.toml --example bench_redraw --features test-support`. It prints median, mean and p90 ms per frame for an idle redraw, the spinner alone, one changed row, a full-screen scroll and a split tab. `BENCH_SCREEN=<file>` replays a recorded screen instead.
- **A gesture that changes the pane area:** the tests that check a slide or drag resizes the shell once also print a `[measure]` line with frames and ms per frame: `cargo test --manifest-path apps/native/Cargo.toml --features test-support --lib once_when_it_settles -- --nocapture`. A new gesture gets the same test.
- **Diff scrolling:** `--example bench_diff_scroll`. **Plugins:** `--example bench_plugins` gives each plugin's memory and time to its first screen.
- **Terminal replay:** `cargo test --release --manifest-path apps/native/Cargo.toml --features test-support --lib -- --ignored --nocapture bench_replay` draws a full ring of recorded Claude Code output (`apps/native/bench/claude-fullscreen.rec`).
- **Polling:** count the calls with a fake in a test, as the tests in `apps/native/src/workspace/usage.rs` do, and check that nothing is asked while Wings is behind other apps.

Don't click through the GUI while the user is using their Mac. Use the offscreen examples and `examples/shot.rs` instead.
