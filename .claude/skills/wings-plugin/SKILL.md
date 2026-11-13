---
name: wings-plugin
description: Creates or changes a Wings plugin, a folder with `wings-plugin.json` and a Rust crate in `rust/` that builds into WebAssembly modules in `dist/native/` (a `main.wasm` and one `.wasm` per page), which run sandboxed inside the Wings app and draw badges, actions, pages and diffs on terminal panes. Use when asked to build a plugin, add a badge, action, page or diff to one, or add a new host call to the plugin SDK.
---

# Wings plugins

The reference is `plugins/README.md`: layout, build, helpers, manifest, permissions and every `wings.*` call. Read it first. The SDK is the `wings-plugin` crate in `packages/plugin-rs`. `plugins/dev-servers/` is the smallest plugin, `plugins/changes/` has a main and two pages, and `plugins/pr-tracker/` is the largest. For anything a page shows, follow the `wings-page-design` skill and render it with `shot_plugins` before calling it done.

## Where the pieces live

| Piece | File |
|---|---|
| SDK a plugin links (`ui`, `host`, `messages`, `cli`, `git`, `tools`...) | `packages/plugin-rs/src` |
| Host: the imports and exports of a frame, its thread and fuel | `apps/native/src/page/runtime/wasm.rs` |
| Host calls a plugin makes (`exec`, `openUrl`, badges, diffs...) and their permission checks | `apps/native/src/page/plugin/calls.rs`, `crates/wings-plugins` |
| Which tree nodes the app draws, and with what props | `apps/native/src/page/tree.rs`, `apps/native/src/page/tree/` |
| Installing, turning on, and starting a plugin's frames | `apps/native/src/installed.rs`, `apps/native/src/workspace/plugin_manage.rs` |
| The committed plugins run against a fake app | `apps/native/src/page/plugin/shipped.rs` |
| Screenshots | `apps/native/examples/shot_plugins.rs` |

## Make a plugin

1. Create `plugins/<id>/wings-plugin.json`. The `id` is lowercase letters, digits and dashes, `api` is `1`, and `minWingsVersion` is at least `0.2.17`, the first Wings that runs `.wasm` frames.
2. Ask for the least you need. `exec` lists commands with their subcommand and the only flags the code passes, like `gh pr view [--json]` (`[]` for none), `transcript` lists Claude Code entry types, and `openUrl` lists https prefixes. Pass a folder with the call's `cwd`, not `git -C`, because the subcommand has to come first, and that folder has to be inside one of the user's Wings projects or a git repo an open pane is in. `plugins/README.md` lists what `exec` refuses whatever a manifest says.
3. Make the crate `plugins/<id>/rust/`: a `cdylib` that depends on `wings-plugin = { path = "../../../packages/plugin-rs" }`, with one Cargo feature per frame (`main`, `sidebar`...). Add it to `members` in `plugins/Cargo.toml`. `lib.rs` pulls in each frame's module with `#[cfg(feature = "<frame>")]`, and the module defines `#[unsafe(no_mangle)] pub fn start()`. Copy `plugins/changes/rust` for the shape.
4. The manifest names the modules: `"main": "dist/native/main.wasm"` and a page as `"page": "dist/native/<frame>.wasm"`. The main is hidden, so its UI goes through badges, pane status, the diff viewer, project sections and the plugin's pages. A page calls `wings_plugin::run(view)` in `start` (`run_filled` for a page that fills its tab edge to edge), and `view` builds its nodes from the page's state with `wings_plugin::ui`. Follow the `wings-page-design` skill for the layout.
5. Declare what it adds in `contributes`: `ui` (`badges`, `diff`), `sidebars`, `views` (pages that open in a modal, a window or a tab, with `host::open_view_with`), `settings`, `blocks` and tools for Claude (`mcpTools` in the manifest, answered with `tools::register` in main, plus an `mcpInstructions` line of up to 500 characters saying when Claude should reach for them: Claude sees only tool names and that line at session start). The host refuses calls the manifest didn't declare. For an API, declare `permissions.fetch` and bind the token to its host in `permissions.secrets`, so plugin code never holds it after setup and it never reaches another host.
6. Build with `sh build-rust.sh` in `plugins/` and commit the `.wasm` files with the source: Wings installs plugin folders from `main`, built files and all. `sh build-rust.sh --check` fails when a committed module differs from what `rust/` builds. It needs `rustup target add wasm32-unknown-unknown`.
7. Loop on it. A debug build of the app loads the repo's `plugins/` folder: `cargo run --manifest-path apps/native/Cargo.toml -- --plugins-dev-root plugins`. A release build needs `wings plugin link <folder>`.

## Rules that keep plugins fast and safe

- Reach for the SDK modules before writing your own (the Helpers table in `plugins/README.md`): `messages` between frames, `settings::Setting`, `cli::Cli`, `git`, `cache::Cached` and `Batch`, `poll::while_focused`, `task::spawn`. No request ids, hello messages or sign-in handling by hand.
- Work from `host::on_panes`. Each pane has a live `cwd` and a `session` while Claude runs in it.
- Follow a session with `sessions::follow` rather than polling `transcript`, which rereads the whole file.
- Keep no map of what you sent. Wings skips a repeated badge or pane status, and drops a closed pane's badge and status, so set them on every pass and send `null` only to clear a pane that stays open. Repaint on `git::on_change` rather than a fast git poll.
- Cache network lookups per repo and branch, not per pane. Local `git` takes about 25 ms and a network CLI 0.7 to 2 s.
- Poll slowly: local checks every 15 s, network every 60 s, and only while Wings is focused, with `poll::while_focused`. Offer a Refresh action instead of polling faster.
- Never make a click wait on the network. Open the diff viewer first and fill it after. Prefetch heavy data after the status loads.
- `exec` has no shell and no stdin. Pass arguments as a list, use program names rather than paths, and never `sh -c`. That keeps it working on macOS, Windows and Linux.
- A frame runs one turn at a time and a turn has a fuel budget (`TURN_FUEL` in `wasm.rs`). Do heavy work in a host call (`exec`, a scan) and not in the module.
- Plugin data is untrusted. A new field a plugin sends the app is bounded in `apps/native/src/page/tree.rs` or in its call's handler.

## Adding a host call

A new call touches these places. Change them together:

1. `packages/plugin-rs/src/host.rs`: the wrapper the plugins call, with its doc comment.
2. `apps/native/src/page/runtime/wasm.rs` and `apps/native/src/page/plugin/calls.rs`: the app's side of it, validating its params.
3. `crates/wings-plugins`: the permission check, with a test.
4. `apps/native/src/page/plugin/shipped.rs`: a test of a committed plugin that makes the call, when one does.

Then update `plugins/README.md`.

## Check it

```sh
cd plugins && sh build-rust.sh --check
cd plugins && cargo test --workspace --quiet
cargo test --manifest-path apps/native/Cargo.toml --features test-support --lib
```

A plugin's own tests run on this computer, where the SDK has no host: nothing is drawn or sent, a call is never answered, the clock stands at 0 and the zone is UTC. `host::testing::answer` answers calls with made-up output, `host::testing::set_now` moves the clock and `local_time::testing::zone` sets a zone. The app's tests use Rust fakes (`apps/native/src/page/runtime/fake.rs`), never a real CLI or network.

Render pages with `shot_plugins`, as the `wings-page-design` skill says. Wings is a GUI the user is often using. Don't click through it yourself. Ask them to check what a render can't show.
