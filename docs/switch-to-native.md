# Switching from the desktop app to the native app

The native app (`apps/native`) replaces the Tauri desktop app (`apps/desktop`). This is how the switch build is made, what it does with your data, how to install it by hand or ship it through the desktop's updater, and how to go back.

## The two native builds

| | Side-by-side (default) | Replacement (`WINGS_SWITCH=1`) |
|---|---|---|
| Build | `sh scripts/build-mac-app.sh` | `WINGS_SWITCH=1 sh scripts/build-mac-app.sh` (cargo feature `switch`) |
| Bundle | `Wings-native.app` | `Wings.app` |
| Bundle id | `dev.wings.native` | `dev.wings.app`, the desktop's |
| Version | `apps/native/Cargo.toml` | the desktop's, from `apps/desktop/src-tauri/Cargo.toml` |
| Data folder | `~/Library/Application Support/Wings Native` | `~/Library/Application Support/dev.wings.app`, the desktop's |
| Its state file | `Wings Native/workspace.json` | `dev.wings.app/native-workspace.json` |
| Update feed | `native-latest-darwin-<arch>.json`, release `native-v<version>` | `wings-latest-darwin-<arch>.json`, release `v<version>`, plus the desktop's `latest.json` |
| Signed with | "Wings Local Signing" | "Wings Local Signing" |

Both bundles hold `wings-native` (the app, `CFBundleExecutable`), `wings` (the command Claude Code runs), `wings-plugin-worker` and `wings-terminal-server`. The folder and the identity are decided in one place, `apps/native/src/data_dir.rs`, and the updater's identity in `apps/native/src/updater.rs` (`APP`).

## What happens to your data

On its first start the replacement runs `data_dir::migrate` (`apps/native/src/data_dir/migrate.rs`) and writes `dev.wings.app/native-migration.json` with a report. Later starts see that file and change nothing, so a project you remove after the switch stays removed.

| File or folder in `dev.wings.app` | Owner after the switch | Native used it before? | Same format? | What the switch does |
|---|---|---|---|---|
| `workspace.json` (tabs, splits, panes' folders and output ids, prefs, Inbox, sessions, `claudeAccounts`) | stays the desktop's | Read only: `activeSpaceId` (first project) and `globalHotkey` | No. Native's file is one entry per project with a split tree; the desktop's is one list of tabs with pane keys into `cwds`, `histories`, `sessions` | Read once and turned into native tabs and settings (`data_dir/desktop.rs`). Never written, so the desktop finds it as it left it |
| `native-workspace.json` | native | new | native's format | Made once: the desktop's tabs and settings, plus projects and settings only the side-by-side app had. The desktop's win where both have one |
| `spaces.json` (projects) | native | Copied once into `Wings Native/spaces.json` on its first run | Yes, both use `wings_spaces::spaces::Space` | Projects only `Wings Native/spaces.json` had are appended; the old file is copied to `backups/native-switch-<ms>/spaces.json` first |
| `scrollback/` (each pane's last output) | shared | No | Yes: raw bytes, same id rule (`[A-Za-z0-9-]{1,64}`) | `Wings Native/scrollback/*` copied in under names not taken. Native's pruning also keeps every id the desktop's `workspace.json` names |
| `plugins/`, `plugins.json` | native | Yes, read and written in place | Yes, `wings-plugins` is the desktop's own crate | Nothing |
| `plugin-storage/` | shared | Yes, in place | Yes, `wings-plugin-storage` | Nothing |
| Keychain items `dev.wings.app.plugin.<plugin>.<scope>` (plugin secrets) | shared | Yes, same service names (`apps/native/src/secrets.rs`) | Yes | Nothing. With the same bundle id and certificate, the items' access list trusts the replacement |
| `projects/` (project pages) | shared | Yes, in place | Yes, `wings_spaces::project_info` | Nothing |
| `mcp.sock` (Claude Code's and OpenCode's MCP socket) | native | Yes, served whenever the desktop isn't serving it | Same protocol (`wings-connect`) | Nothing. Claude Code's config runs `/Applications/Wings.app/Contents/MacOS/wings --mcp ~/Library/Application Support/dev.wings.app/mcp.sock`, and the replacement has `wings` at that path |
| `claude-disconnected`, `opencode-disconnected` | native | Side-by-side keeps its own beside its state file | Empty marker files | Nothing. The replacement's state file is in `dev.wings.app`, so it reads the desktop's markers. The side-by-side app's markers are not carried over |
| `ai/` (`ai.json`, `ai-names.json`, the `describe-*` helper) | stays the desktop's | No | n/a | Nothing. Native has no AI session naming yet |
| `ptyd/` (the desktop's terminal host, `WINGS_PTYD=1`) | stays the desktop's | No | n/a | Nothing. Native's terminal server uses `terminal-server/` |
| `terminal-server/` | native | `Wings Native/terminal-server/` | n/a | Not moved: it holds a running server's sockets and a copy of its binary, which the replacement makes again in `dev.wings.app/terminal-server/`. The replacement doesn't reattach panes still running in the side-by-side app's server, so quit what runs there first |
| `.window-state.json`, `cli-prompted` | stay the desktop's | No | n/a | Nothing |

The `Wings Native` folder is only read. Nothing either app kept is deleted.

## Before the switch

`main` (desktop 0.1.121 to 0.1.124) is merged into `native`, with per-account usage limits in `crates/wings-connect`. Still to do:

- **Blockers.**
  - Bump the desktop's version (`apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/package.json`, `apps/desktop/src-tauri/Cargo.toml`) to 0.2.0 (decided), above the last desktop release. The replacement takes its version from there, and the desktop's updater offers only a higher version than its own 0.1.124.
- **Follow-ups**, which the desktop does and native doesn't yet:
  - AI session naming (`ai/`), and saving Project page tabs.

## Switching this Mac by hand

Run these from Terminal.app, not from a pane in Wings: quitting Wings ends the shells in it, including Claude Code's.

1. Check that the desktop's old terminal host isn't registered. It runs `Contents/MacOS/wings ptyd` from the bundle, which in the replacement is the `wings` command, so launchd would restart it forever:
   ```sh
   launchctl print gui/$(id -u)/dev.wings.ptyd
   ```
   It must say it could not find the service. If it finds it, open the desktop app once without `WINGS_PTYD=1`, which unregisters it.
2. Build, from `apps/native`: `WINGS_SWITCH=1 sh scripts/build-mac-app.sh`. Check the signature: `codesign -dr - target/release/Wings.app` must print `identifier "dev.wings.app" and certificate leaf = H"..."` with the same hash as `codesign -dr - /Applications/Wings.app`.
3. Back up the desktop app and its data folder:
   ```sh
   B=~/WingsSwitchBackup/$(date +%Y%m%d-%H%M%S); mkdir -p "$B"
   ditto /Applications/Wings.app "$B/Wings.app"
   rsync -a --exclude mcp.sock ~/Library/Application\ Support/dev.wings.app/ "$B/dev.wings.app/"
   rsync -a --exclude '*.sock' ~/Library/Application\ Support/Wings\ Native/ "$B/Wings Native/"
   ```
4. Quit both apps: `osascript -e 'tell application id "dev.wings.app" to quit' -e 'tell application id "dev.wings.native" to quit'`, and wait until `pgrep -fl 'Wings.app/Contents/MacOS|Wings-native.app/Contents/MacOS'` prints nothing.
5. Install: `rm -rf /Applications/Wings.app && ditto target/release/Wings.app /Applications/Wings.app`. Keep `/Applications/Wings-native.app` until the checks pass, but don't open it: it would serve the socket if it starts first.
6. Open it: `open /Applications/Wings.app`.

## Shipping it through the desktop's updater

1. From `apps/native`: `WINGS_SWITCH=1 sh scripts/package-mac-app.sh "Wings is now native"`. In `target/release/bundle/macos/darwin-<arch>/` it writes `Wings-<version>-darwin-<arch>.app.tar.gz` and its `.sig`, `wings-latest-darwin-<arch>.json` and its `.sig`, and `latest.json`.
2. Publish all five in a release tagged `v<version>` in `josippapez/Wings`, and make it the latest release.
3. A desktop install finds `latest.json`, verifies the archive with the same updater key, and swaps the bundle in the archive in place of its own `Wings.app` (Tauri's updater skips the archive's top folder). Its restart reads the new `Info.plist` and starts `CFBundleExecutable`, `wings-native`. From then on the replacement's own updater checks `wings-latest-darwin-<arch>.json`.
   The desktop has no terminal server, so its terminals end at this restart as on any desktop update. The replacement starts its server on its first launch, and from then on its own updater's restarts keep terminals running (see [releasing.md](releasing.md)).
4. Later releases are made the same way. Keep publishing `latest.json` for desktops that haven't updated yet. Don't make a side-by-side `native-v*` release the latest one while desktops may still update, since it has no `latest.json`.

## First launch checks

- [ ] `~/Library/Application Support/dev.wings.app/native-migration.json` exists and its `report` counts look right (projects from the desktop, from the side-by-side app, spaces added, outputs copied). The app's stderr also prints it once (`log stream --process wings-native` or run `/Applications/Wings.app/Contents/MacOS/wings-native` from Terminal).
- [ ] The sidebar lists every project the desktop listed, plus any you added only in the side-by-side app.
- [ ] Each project opens the tabs and splits the desktop had, each pane in its folder with its last output above a new shell. Plugin pages that were open come back. Project page tabs don't (native doesn't save them).
- [ ] Settings show the desktop's font size, cursor, scrollback, Option as Meta, global shortcut, quick terminal, sounds, sort order and "Let Claude run commands" / "Let agents message each other".
- [ ] Plugins are on as they were, and none asks for approval again. A plugin with a saved secret (a token) still works without a keychain prompt.
- [ ] `wings plugin list` in a pane answers (the socket is served). In Claude Code, `/mcp` shows `wings` connected, and a Wings tool such as `list_sessions` works. If the socket isn't served (the desktop had not let go of it yet after an updater restart), quit and reopen Wings.
- [ ] Settings, Agents shows Claude Code connected in each account, without changing `~/.claude.json`: `jq .mcpServers.wings ~/.claude.json` still runs `/Applications/Wings.app/Contents/MacOS/wings`.
- [ ] No new macOS privacy prompt for folders you already allowed (Desktop, Documents, Downloads, Full Disk Access, Automation). Approvals are keyed by bundle id and certificate, which match. Panes run in the terminal server by default (Settings > General > "Keep terminals running when Wings quits"; `WINGS_TERMINAL_SERVER=0` or `=1` overrides it), under `wings-terminal-server`'s own signature, so they ask once, as they did in the side-by-side app.
- [ ] The global shortcut and the quick terminal key work.
- [ ] Check for Updates answers. Until a release has `wings-latest-darwin-<arch>.json` it reports an error, which is expected.
- [ ] `launchctl print gui/$(id -u)/dev.wings.ptyd` still finds nothing.

## Going back

1. Quit Wings. Restore the desktop app: `rm -rf /Applications/Wings.app && ditto "$B/Wings.app" /Applications/Wings.app`.
2. The desktop's `workspace.json` is as it was at the switch, so its tabs come back as they were then. Tabs and settings you changed in the native app are not carried back.
3. Projects, plugins, plugin data and secrets, and project pages changed in the native app carry back: they are the same files in the same formats. To undo the projects the switch appended, copy `dev.wings.app/backups/native-switch-<ms>/spaces.json` over `dev.wings.app/spaces.json`. To restore everything, copy the data folder back from `$B/dev.wings.app` while no Wings runs.
4. `native-workspace.json` and `native-migration.json` stay, and the desktop ignores them. The desktop deletes saved outputs its own tabs don't name, so switching again later restores native panes without their old output. Delete `native-migration.json` before switching again only if you want the migration to run again; it then keeps the existing `native-workspace.json`.

## Decision: keep `dev.wings.app`

The replacement uses the desktop's bundle id. That is what makes the switch invisible:

- macOS privacy approvals (TCC) and keychain access lists match an app by its designated requirement, `identifier "dev.wings.app" and certificate leaf = H"c68ca937..."`. The replacement has the same one, so approvals and plugin secrets carry over.
- Tauri names the desktop's data folder after the id, and `wings-connect` finds `mcp.sock` there (`IDENTIFIER`), so Claude Code, OpenCode and the journal sink need no change.
- The desktop's updater replaces `/Applications/Wings.app` with whatever bundle the archive holds, so it delivers the replacement to every desktop install.

A new id (for example keeping `dev.wings.native`) would ask for every privacy approval and every keychain item again, keep two data folders, and rely on the side-by-side app's `Wings Native` folder. The desktop's updater would still install it in `Wings.app`'s place, but macOS would see a different app.
