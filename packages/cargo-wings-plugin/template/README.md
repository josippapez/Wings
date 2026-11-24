# Hello Wings

A starting point for a Wings plugin written in Rust that lives in its own repo. It puts a badge with the folder on each
terminal pane, lists the panes in a sidebar, and has a tool Claude can call.

- `wings-plugin.json` is the manifest: what the plugin is, what it asks for and what it adds. `main` and each page name
  the WebAssembly module Wings runs.
- `rust/` is the crate. One Cargo feature per frame: `main` (`src/main_frame.rs`, hidden, sets the badges and answers
  the tool) and `sidebar` (`src/sidebar.rs`, the page). A new page is a new feature, a `#[cfg(feature = "...")]` module
  in `src/lib.rs`, and a `.wasm` named in `contributes`. The SDK is the `wings-plugin` crate, which
  `rust/Cargo.toml` takes from crates.io. Its API is on [docs.rs](https://docs.rs/wings-plugin) and in
  [plugins/README.md](https://github.com/josippapez/Wings/blob/main/plugins/README.md) of the Wings repo.

## Work on it

You need Rust, the WebAssembly target and the author commands:

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-wings-plugin
```

```sh
cargo wings-plugin build   # builds dist/native/main.wasm and sidebar.wasm
cargo wings-plugin check   # the built modules match rust/, and the manifest names files that exist
cargo wings-plugin dev     # rebuilds on each save and has the running Wings restart the plugin
cargo wings-plugin pack    # writes hello-wings.wings-plugin: the manifest, README, icon and dist/
```

`cargo test --manifest-path rust/Cargo.toml --all-features` runs the crate's tests on your computer, every frame's
together: each frame's `start` is exported only outside tests (`cfg_attr(not(test), unsafe(no_mangle))`), so they
don't clash.

`dev` runs `wings plugin link <folder>` after each build, which links this folder into the running Wings. Wings asks
you to approve the plugin's access the first time.

A page can't be screenshotted outside the Wings repo: run the plugin in Wings with `dev` and look at it there.

## Try it in Wings

Pack it and drop the file on the Wings window, or pick it with Plugins, Choose a file. Wings asks you to approve the
plugin's access before it runs.

## Publish it

Replace this README (Wings shows a plugin's README on its page in Settings) and the icon, set your own `description`,
and commit everything but `dist/` and `rust/target/` (`.gitignore` leaves them out; the release workflow builds
`dist/`). Bump `version` in `wings-plugin.json`, commit, then tag and push:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The release workflow builds the plugin and attaches `<id>.wings-plugin` to a GitHub release. Anyone can then install it
by pasting the repo link into Plugins, and Wings offers the update when a new release comes out. The repo has to be
public for that. For a private plugin, share the `.wings-plugin` file instead.
