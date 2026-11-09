# Releasing Wings

Wings checks the latest GitHub release on start and every six hours. When it finds a newer version it shows a toast, "Wings X.Y.Z is available", with an "Install and restart" button. It uses Tauri's updater plugin, which only installs archives signed with the updater key.

## One-time setup

The updater key pair signs each release. The public half is in `apps/desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private half lives at `~/.tauri/wings-updater.key`, outside the repo. Back it up somewhere private: if it is lost, installed copies can't verify updates and everyone has to reinstall by hand. Never commit it.

To make a new pair: `pnpm tauri signer generate -w ~/.tauri/wings-updater.key`, then put the new public key in `tauri.conf.json`. Releases signed with the new key are only accepted by builds that already carry the new public key.

You also need the local code-signing certificate from [signing.md](signing.md) and the `gh` CLI signed in.

## Steps

1. Bump `version` in `apps/desktop/src-tauri/tauri.conf.json` (and `apps/desktop/package.json`, `apps/desktop/src-tauri/Cargo.toml` to match). Commit and merge it.
2. `cd apps/desktop && pnpm release:mac "What changed, one line"`. It builds the signed app and writes, in `apps/desktop/src-tauri/target/release/bundle/macos/`, `Wings.app.tar.gz`, `Wings.app.tar.gz.sig` and `latest.json`. Before it builds, it stops when the tree has uncommitted changes, when the app's tests or the Rust tests fail, or when `pnpm check` in `plugins/` finds a committed `dist/` that no longer matches its `src/`, so build and commit it first. Then it packs each folder in `plugins/` into `<id>.wings-plugin` there with `wings-plugin pack`, which builds `src/` and leaves the source and fixtures out (`scripts/pack-plugins.sh`).
3. Run the `gh release create vX.Y.Z ...` command it prints. That uploads the files and publishes the release; running copies find it through `https://github.com/josippapez/Wings/releases/latest/download/latest.json`. The `<id>.wings-plugin` files are for installing a plugin from a file. The command ends with `cargo clean --release --workspace`, and `release:mac` ran `cargo clean --profile dev --workspace` after building. Each version compiles a new copy of the app's own crate, which piled up tens of GB between releases, so both drop every copy of it and keep the compiled dependencies: the next release compiles only the app. When `Cargo.lock` changed for more than the app's version, `release:mac` starts from `cargo clean`, so old dependency versions go too.

Plugins don't need a release. Wings installs and updates `github.com/josippapez/Wings/plugins/<id>` from `main`, and offers an update when the plugin's `version` in `wings-plugin.json` goes up. To ship a plugin change, bump that version and push. If the change needs something only a newer Wings has, also set `minWingsVersion` to that Wings version, so older apps don't install it.

The version in `latest.json` must be higher than the running app's, or nothing is offered. `release:mac` only builds for the Mac architecture it runs on (`darwin-aarch64` on Apple silicon).

## Gatekeeper

The app is signed with the self-signed "Wings Local Signing" certificate, not an Apple Developer ID, and isn't notarized. The updater doesn't care, but a Mac that never ran Wings before will show a Gatekeeper warning on first open (right-click, Open). A Developer ID certificate and notarization would remove it.

## Native macOS updates

This is separate from the desktop updater. Bump `apps/native/Cargo.toml` to a newer stable semantic version. From `apps/native`, run `sh scripts/package-mac-app.sh "Release notes"`. It requires the same updater private key as the desktop (override its path with `WINGS_UPDATER_KEY`). No private key is embedded in the app. The script builds but never installs, quits an app, or publishes.

Under `target/release/bundle/macos/darwin-<architecture>/`, publish these five assets yourself to a release tagged `native-v<version>` in `josippapez/Wings`:

- `Wings-native-<version>-darwin-<architecture>.app.tar.gz`
- `native-latest-darwin-<architecture>.json`
- `native-latest-darwin-<architecture>.json.sig`
- `native-latest-darwin-<architecture>.v2.json`
- `native-latest-darwin-<architecture>.v2.json.sig`

Make it the latest release. Include both architecture feeds when shipping both architectures. A later desktop-only latest release without these assets makes native checks report an error until a latest release includes them again.

The app fetches `https://github.com/josippapez/Wings/releases/latest/download/native-latest-darwin-<architecture>.json` and `.json.sig` over HTTPS. The `.sig` is the Tauri CLI's base64-wrapped minisign signature over the **exact complete JSON bytes**, including the final newline. The signed object has `bundle_identifier` (`dev.wings.native`), native `version`, `platform` (`darwin-aarch64` or `darwin-x86_64`), `artifact` (the basename above), compressed `size` in bytes, lowercase hexadecimal `sha256`, and `notes`. The `.v2.json` feed is the same object plus `server_version`, the terminal server the release brings (`SERVER_VERSION` in `crates/wings-ptyd/src/lib.rs`), which the update dialog compares with the running server's to say what happens to open terminals. It is a second file because apps up to 0.2.10 refuse a feed with a field they don't know; newer apps read it first and fall back to the first feed when a release lacks it. No URLs, destinations or commands come from the feed. Archives are fetched only from `https://github.com/josippapez/Wings/releases/download/native-v<version>/<artifact>`.

The app authenticates before parsing, rejects older/current releases and wrong product/architecture, verifies archive size and SHA-256 before extraction, refuses links and unsafe tar paths, checks the plist and required executables, and runs macOS code-signature verification. Signing failures leave no publishable feed. Native metadata and artifacts never reuse desktop `latest.json` or `Wings.app.tar.gz`.

The replacement build that takes the desktop's place is packaged with `WINGS_SWITCH=1` and published differently: see [switch-to-native.md](switch-to-native.md).

Install and restart is explicit. The trusted running executable copies itself into a private `.wings-update-*` directory beside the destination, waits for normal app shutdown, then uses an atomic macOS directory exchange. The destination stays usable even if the helper is interrupted. Unsupported filesystems fail without a non-atomic fallback. A relaunch failure exchanges the original app back and reopens it with the same arguments. Both launches require startup confirmation. If recovery also cannot open the original app, launch the installed bundle manually. Failed/interrupted helper staging is retained beside it with `install.log` and the other bundle for manual recovery. Do not remove it while an installer is running. Successful installs remove staging. Terminals in the terminal server (on by default, Settings > General > "Keep terminals running when Wings quits") keep running through the restart and the new app attaches them again, also when the release ships a newer `SERVER_VERSION` (the old server drains: see [apps/native/README.md](../apps/native/README.md#terminals-that-outlive-the-app)). Terminals running inside the app and their agents may stop.
