# wings-plugin

The SDK for writing a [Wings](https://github.com/josippapez/Wings) plugin in Rust. Wings is a terminal app for macOS;
a plugin is a Rust crate built to WebAssembly (`wasm32-unknown-unknown`) that Wings runs in a sandbox, with the
permissions its manifest asks for. It adds badges to terminal panes, pages (sidebars, tabs, blocks, settings), a diff
viewer, tools for Claude, and hooks.

This crate is what a plugin links: `host` (the calls to Wings), `ui` (the nodes a page is made of), `messages` between a
plugin's frames, `tools`, `cli`, `git`, `poll`, `cache` and more.

## Start a plugin

```sh
cargo install cargo-wings-plugin
rustup target add wasm32-unknown-unknown
cargo wings-plugin new my-plugin
cd my-plugin
cargo wings-plugin dev      # rebuilds on each save and has the running Wings restart the plugin
cargo wings-plugin pack     # writes my-plugin.wings-plugin, the file Wings installs
```

## A minimal plugin

`rust/Cargo.toml`:

```toml
[package]
name = "hello"
version = "0.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
wings-plugin = "0.1"

[features]
# One Cargo feature per frame: dist/native/sidebar.wasm is built with `--features sidebar`.
sidebar = []
```

`rust/src/lib.rs`, a sidebar page:

```rust
#[cfg(feature = "sidebar")]
mod sidebar {
    use wings_plugin::ui::{Node, text};

    fn view() -> Vec<Node> {
        vec![text("Hello from a plugin")]
    }

    #[cfg_attr(not(test), unsafe(no_mangle))]
    pub fn start() {
        wings_plugin::run(view);
    }
}
```

`wings-plugin.json` names the modules: `"main": "dist/native/main.wasm"` (a hidden frame that runs while the plugin is
on, built from a `main` feature the same way) and
`"contributes": { "sidebars": [{ "id": "hello", "title": "Hello", "page": "dist/native/sidebar.wasm" }] }`.
`cargo wings-plugin new` writes a complete plugin, with a main, a page and a tool for Claude.

Built for your own computer, the crate has no host: nothing is drawn or sent, so a plugin's logic can be tested with
`cargo test`, and `host::testing::answer` answers its calls with made-up output.

## Documentation

- The API reference is on [docs.rs](https://docs.rs/wings-plugin).
- The manifest, permissions, layout, helpers and every host call are in
  [plugins/README.md](https://github.com/josippapez/Wings/blob/main/plugins/README.md).
- The plugins in the [Wings repo](https://github.com/josippapez/Wings/tree/main/plugins) are working examples.

While the version is `0.x`, a minor release can change the API.

## License

MIT.
