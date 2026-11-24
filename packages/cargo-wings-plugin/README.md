# cargo-wings-plugin

`cargo wings-plugin`: the author commands for [Wings](https://github.com/josippapez/Wings) plugins written in Rust. A
plugin is a folder with a `wings-plugin.json` and a Rust crate in `rust/` that builds to WebAssembly; the SDK it links
is the [`wings-plugin`](https://docs.rs/wings-plugin) crate.

```sh
cargo install cargo-wings-plugin
rustup target add wasm32-unknown-unknown
```

| Command | What it does |
| --- | --- |
| `cargo wings-plugin new <name> [--sdk <path>]` | Starts a plugin in a new folder, with a main, a sidebar page and a tool. `--sdk` takes the SDK from a Wings checkout's `packages/plugin-rs` instead of crates.io. |
| `cargo wings-plugin build` | Builds each module the manifest names (one Cargo feature per frame, named like the file) into `dist/native/`. |
| `cargo wings-plugin check` | Fails when a built module differs from what `rust/` builds now, or the manifest names a file that isn't there. |
| `cargo wings-plugin pack` | Zips what the plugin ships into `<id>.wings-plugin`, the file Wings installs. Run `build` first. |
| `cargo wings-plugin dev` | Rebuilds on each save and runs `wings plugin link <folder>`, so the running Wings restarts the plugin. |

Share a plugin as a GitHub repo: tag a release, and the template's workflow attaches the `.wings-plugin` file. Pasting the
repo link into Plugins, Add plugin in Wings installs it. The API is on [docs.rs](https://docs.rs/wings-plugin) and in
[plugins/README.md](https://github.com/josippapez/Wings/blob/main/plugins/README.md).

`dev` needs the `wings` command, from the app (`/Applications/Wings.app/Contents/MacOS/wings`) or on your PATH.

## License

MIT.
