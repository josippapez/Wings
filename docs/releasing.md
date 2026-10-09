# Releasing Wings

Wings checks the latest GitHub release on start and every six hours. When it finds a newer version it shows a toast, "Wings X.Y.Z is available", with an "Install and restart" button. It uses Tauri's updater plugin, which only installs archives signed with the updater key.

## One-time setup

The updater key pair signs each release. The public half is in `apps/desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private half lives at `~/.tauri/wings-updater.key`, outside the repo. Back it up somewhere private: if it is lost, installed copies can't verify updates and everyone has to reinstall by hand. Never commit it.

To make a new pair: `pnpm tauri signer generate -w ~/.tauri/wings-updater.key`, then put the new public key in `tauri.conf.json`. Releases signed with the new key are only accepted by builds that already carry the new public key.

You also need the local code-signing certificate from [signing.md](signing.md) and the `gh` CLI signed in.

## Steps

1. Bump `version` in `apps/desktop/src-tauri/tauri.conf.json` (and `apps/desktop/package.json`, `apps/desktop/src-tauri/Cargo.toml` to match). Commit and merge it.
2. `cd apps/desktop && pnpm release:mac "What changed, one line"`. It builds the signed app and writes, in `apps/desktop/src-tauri/target/release/bundle/macos/`, `Wings.app.tar.gz`, `Wings.app.tar.gz.sig` and `latest.json`.
3. Run the `gh release create vX.Y.Z ...` command it prints. That uploads the three files and publishes the release; running copies find it through `https://github.com/josippapez/Wings/releases/latest/download/latest.json`.

The version in `latest.json` must be higher than the running app's, or nothing is offered. `release:mac` only builds for the Mac architecture it runs on (`darwin-aarch64` on Apple silicon).

## Gatekeeper

The app is signed with the self-signed "Wings Local Signing" certificate, not an Apple Developer ID, and isn't notarized. The updater doesn't care, but a Mac that never ran Wings before will show a Gatekeeper warning on first open (right-click, Open). A Developer ID certificate and notarization would remove it.
