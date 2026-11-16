# Releasing Wings

Wings is the native app in `apps/native`. It checks the latest GitHub release on start and every six hours, and when it finds a newer version it offers to install it and restart. It only installs releases signed with the updater key.

## One-time setup

The updater key pair signs each release. The public half is in `apps/desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`), which the native app reads when it's built. The private half lives at `~/.tauri/wings-updater.key`, outside the repo. Back it up somewhere private: if it is lost, installed copies can't verify updates and everyone has to reinstall by hand. Never commit it.

To make a new pair: `apps/desktop/node_modules/.bin/tauri signer generate -w ~/.tauri/wings-updater.key`, then put the new public key in `tauri.conf.json`. Releases signed with the new key are only accepted by builds that already carry the new public key.

You also need the local code-signing certificate from [signing.md](signing.md), `pnpm install` run once in `apps/desktop` for the Tauri CLI that signs releases, and the `gh` CLI signed in.

## Steps

1. Bump `version` in `apps/native/Cargo.toml` and add its section to `CHANGELOG.md`. Run the tests: `cargo test --manifest-path apps/native/Cargo.toml --features test-support --lib`. Commit and push to `main`.
2. From `apps/native`: `WINGS_SWITCH=1 sh scripts/package-mac-app.sh "What changed, one line"`. It builds and signs `Wings.app` and writes seven files to `target/release/bundle/macos/darwin-<architecture>/`: `Wings-<version>-darwin-<architecture>.app.tar.gz` and its `.sig`, `wings-latest-darwin-<architecture>.json` and `.v2.json` with their `.sig`s, and `latest.json` for desktop installs that haven't switched yet. It doesn't run the tests or check the tree.
3. Publish all seven to a release tagged `v<version>`, with the version's `CHANGELOG.md` section as its notes:

   ```sh
   gh release create v<version> <the seven files> --repo josippapez/Wings --target main --title "Wings <version>" --notes-file <notes.md> --latest
   ```

Plugins don't need a release. Wings installs and updates `github.com/josippapez/Wings/plugins/<id>` from `main`, and offers an update when the plugin's `version` in `wings-plugin.json` goes up. To ship a plugin change, bump that version and push. If the change needs something only a newer Wings has, also set `minWingsVersion` to that Wings version, so older apps don't install it.

The version must be higher than the running app's, or nothing is offered. The script only builds for the Mac architecture it runs on (`darwin-aarch64` on Apple silicon).

## The desktop app is deprecated

`apps/desktop`, the earlier Tauri app, is deprecated and no longer released. Don't bump its version or run its `pnpm release:mac`. Copies of it still installed update through the `latest.json` each release carries, which installs the native app in its place. The native app still reads a few files from `apps/desktop` (the updater public key, the Tauri CLI that signs releases, the hotkey command list and some test fixtures), so the folder stays until those move.

## Gatekeeper

The app is signed with the self-signed "Wings Local Signing" certificate, not an Apple Developer ID, and isn't notarized. The updater doesn't care, but a Mac that never ran Wings before will show a Gatekeeper warning on first open (right-click, Open). A Developer ID certificate and notarization would remove it.

## The side-by-side app's updates

`Wings-native.app` (`dev.wings.native`), the build that runs beside the desktop app, has a feed of its own. From `apps/native`, run `sh scripts/package-mac-app.sh "Release notes"`. It requires the same updater private key as the desktop (override its path with `WINGS_UPDATER_KEY`). No private key is embedded in the app. The script builds but never installs, quits an app, or publishes.

Under `target/release/bundle/macos/darwin-<architecture>/`, publish these five assets yourself to a release tagged `native-v<version>` in `josippapez/Wings`:

- `Wings-native-<version>-darwin-<architecture>.app.tar.gz`
- `native-latest-darwin-<architecture>.json`
- `native-latest-darwin-<architecture>.json.sig`
- `native-latest-darwin-<architecture>.v2.json`
- `native-latest-darwin-<architecture>.v2.json.sig`

Make it the latest release. Include both architecture feeds when shipping both architectures. A later desktop-only latest release without these assets makes native checks report an error until a latest release includes them again.

The app fetches `https://github.com/josippapez/Wings/releases/latest/download/native-latest-darwin-<architecture>.json` and `.json.sig` over HTTPS. The `.sig` is the Tauri CLI's base64-wrapped minisign signature over the **exact complete JSON bytes**, including the final newline. The signed object has `bundle_identifier` (`dev.wings.native`), native `version`, `platform` (`darwin-aarch64` or `darwin-x86_64`), `artifact` (the basename above), compressed `size` in bytes, lowercase hexadecimal `sha256`, and `notes`. The `.v2.json` feed is the same object plus `server_version`, the terminal server the release brings (`SERVER_VERSION` in `crates/wings-ptyd/src/lib.rs`), which the update dialog compares with the running server's to say what happens to open terminals. It is a second file because apps up to 0.2.10 refuse a feed with a field they don't know; newer apps read it first and fall back to the first feed when a release lacks it. No URLs, destinations or commands come from the feed. Archives are fetched only from `https://github.com/josippapez/Wings/releases/download/native-v<version>/<artifact>`.

The app authenticates before parsing, rejects older/current releases and wrong product/architecture, verifies archive size and SHA-256 before extraction, refuses links and unsafe tar paths, checks the plist and required executables, and runs macOS code-signature verification. Signing failures leave no publishable feed. Native metadata and artifacts never reuse desktop `latest.json` or `Wings.app.tar.gz`.

The release in Steps above is the replacement build, `WINGS_SWITCH=1`: see [switch-to-native.md](switch-to-native.md).

Install and restart is explicit. The trusted running executable copies itself into a private `.wings-update-*` directory beside the destination, waits for normal app shutdown, then uses an atomic macOS directory exchange. The destination stays usable even if the helper is interrupted. Unsupported filesystems fail without a non-atomic fallback. A relaunch failure exchanges the original app back and reopens it with the same arguments. Both launches require startup confirmation. If recovery also cannot open the original app, launch the installed bundle manually. Failed/interrupted helper staging is retained beside it with `install.log` and the other bundle for manual recovery. Do not remove it while an installer is running. Successful installs remove staging. Terminals in the terminal server (on by default, Settings > General > "Keep terminals running when Wings quits") keep running through the restart and the new app attaches them again, also when the release ships a newer `SERVER_VERSION` (the old server drains: see [apps/native/README.md](../apps/native/README.md#terminals-that-outlive-the-app)). Terminals running inside the app and their agents may stop.
