---
name: wings-plugin
description: Creates or changes a Wings plugin, a folder with `wings-plugin.json` and TypeScript in `src/` that builds into `dist/` (a `dist/main.js` and React pages), which run sandboxed inside the Wings desktop app and draw badges, actions and diffs on terminal panes. Use when asked to build a plugin, add a badge, action or diff to one, or add a new capability to the plugin API.
---

# Wings plugins

The API reference is `plugins/README.md`: manifest, permissions, every `wings.*` call, and the badge and comment shapes. Read it first. `plugins/dev-servers/` (one script, no pages) and the starter that `wings-plugin init` copies (`packages/plugin-sdk/template/`: a badge and a sidebar, on the SDK) are the small examples to copy from; `plugins/pr-tracker/` is the largest, with five hosts. For anything a page shows, follow the `wings-page-design` skill and render it with `pnpm --dir plugins shot` before calling it done.

## Where the pieces live

| Piece | File |
|---|---|
| SDK loaded into the plugin frame (`window.wings`) | `apps/desktop/public/plugin-sdk.js` |
| Plugin page components and the stylesheet Wings inlines into every page | `packages/plugin-ui` (`ui.js`, `plugin-ui.css`) |
| Host: runs frames, relays calls, cleans badges and comments | `apps/desktop/src/lib/plugins.ts` |
| View windows: relay between a window's frame and the host, as events | `apps/desktop/src/lib/plugin-views.ts`, `src/view-window.tsx`, `plugin_view_open` in `lib.rs`, `capabilities/plugin-view.json` |
| Permission checks, `exec`, transcript reads | `apps/desktop/src-tauri/src/plugins.rs` |
| Tauri commands `plugin_exec`, `plugin_transcript`, `plugin_open_url`, `plugin_ports` | `apps/desktop/src-tauri/src/lib.rs` |
| Live transcript entries for `onTranscript`, read each detection tick | `apps/desktop/src-tauri/src/tail.rs`, `tail_transcripts` in `lib.rs` |
| Claude Code's status line data for `wings.statusline()`, sent by `wings statusline` | `apps/desktop/src-tauri/src/statusline.rs`, `statusline` in `cli.rs` and `control.rs` |
| Badge pill and card | `apps/desktop/src/components/pane-badge.tsx` |
| Diff viewer | `apps/desktop/src/components/diff-viewer.tsx` |

## Make a plugin

1. Create `plugins/<id>/wings-plugin.json`. The `id` is lowercase letters, digits and dashes, and `api` is `1`.
2. Ask for the least you need. `exec` lists commands with their subcommand and the only flags the code passes, like `gh pr view [--json]` (`[]` for none, Wings 0.1.108), `transcript` lists Claude Code entry types, and `openUrl` lists https prefixes. Pass a folder with `{ cwd }`, not `git -C`, because the subcommand has to come first, and that folder has to be inside one of the user's Wings projects or a git repo an open pane is in (git needs one). `plugins/README.md` lists what `exec` refuses whatever a manifest says.
3. Write `src/main.ts` against `window.wings` only, and each page as `src/pages/<page>.tsx` in React with `wings-plugin-ui`: it default-exports its component and says where it shows with `export const page = { place: "sidebar" }` (`sidebar`, `popover`, `view`, `block`, `tab` or `settings`), and the build writes the mount (`showSidebar`, `showBlock` and so on) and `dist/<page>.html`. Build the page from the components (`Card`, `Heading`, `List`, `Gauge`, `Chips`...) and keep only what's genuinely its own in an imported `src/pages/<page>.css`. Don't write the page's HTML or layout CSS: Wings' `plugin-ui.css` styles the `w-*` classes. A page that draws itself, like a canvas, is a `.ts` page that uses `mountShell` from `wings-plugin-sdk/shell`. The main frame is hidden, so its UI goes through badges, the diff viewer and the plugin's pages. The rest of `src/` is `ui/` (React components the pages share), `lib/` (logic with no UI, messages and settings, and their tests) and `main/` (the modules a large main script splits into). The manifest points into `dist/`: `"main": "dist/main.js"` and `"page": "dist/<page>.html"`. Run `pnpm build` in `plugins/` and commit `dist/` with the source: Wings installs plugin folders from `main`, built files and all, and `pnpm check` fails when a committed file differs from what `src/` builds. A plugin in its own repo ships with `wings-plugin pack` instead. See Writing a plugin in `plugins/README.md` and `packages/plugin-ui/README.md`.
4. Declare what it adds in `contributes`: `ui` (`badges`, `diff`), `sidebars` (its own pages in the right sidebar, each with an optional `popover` page shown when you point at its button), `views` (pages that can also open in a modal, a window of their own or a tab, with `wings.openView`), `panels` (outside web pages in a popover), `settings` (its own page in Wings Settings, opened with `wings.openSettings`) `blocks` (pages the user places on a project's Project page; give the block its own page with `place: "block"`, which the build mounts with `showBlock` so the block keeps the page's height; `wings.placement()` is `"block"`, with `blockState`/`setBlockState` for per-block state; `wings.focused` is false while it is out of view; needs `"minWingsVersion": "0.1.24"`) and any tools for Claude (define them in `src/tools.ts` with `tool()` and `s` and answer them with `registerTools(tools)` in main: the build writes `mcpTools` from that file), with an `mcpInstructions` line (up to 500 characters) saying when Claude should reach for them: Claude sees only tool names and that line at session start, never the descriptions. The host refuses UI calls the manifest didn't declare. Each tool gets an input schema and a handler in `main`; the build fails when the two disagree. Claude sees it as `<id>__<name>` once the user clicks Connect under Plugins, Claude Code. For an API, declare `permissions.fetch` and bind the token to its host in `permissions.secrets` (`{ "name": "https://api.example.com/" }`), keep it with `wings.secrets.set` and send it with `wings.fetch(url, { bearer })`, so plugin code never holds it after setup and it never reaches another host.
5. Loop on it. In the plugin's folder, `wings-plugin dev` (in this repo `../node_modules/.bin/wings-plugin dev` from `plugins/<id>`, elsewhere `npx wings-plugin dev`) builds on each save and has the running Wings restart the plugin, so there is no restart by hand. Debug builds (`pnpm tauri dev` in `apps/desktop`) load the repo's `plugins/` folder at startup, always on; a release build needs `wings plugin link <folder>`, which `dev` does for you the first time. Look at pages with `pnpm --dir plugins shot <plugin>` in this repo, or `npx wings-plugin shot` in a plugin's own folder (the header of `packages/plugin-sdk/shot.mjs` lists the fixture fields). A plugin in its own repo starts with `npx wings-plugin-sdk init <folder>` (`--pm` picks the package manager, `--no-install` skips the install) and installs through the Plugins manager as a `.wings-plugin` file or a GitHub link.

## Rules that keep plugins fast and safe

- Reach for the SDK helpers before writing your own (the Helpers table in `plugins/README.md`): `channel` for main and the pages (a query for data a page shows, `ask` for a command), `setting`, `cli`, `git`, `cached`, `singleFlight`, `mapLimit`, `latestOnly`, `format`. No request ids, hello messages, sign-in handling or `fitJson` by hand.
- Work from `wings.onPanes`. Each pane has a live `cwd` and a `session` while Claude runs in it.
- Follow a session with `followSessions` from `wings-plugin-sdk/sessions` (or `wings.onTranscript`) rather than polling `wings.transcript`, which rereads the whole file.
- Keep no map of what you sent. Wings skips a repeated `setBadge`, `setPaneStatus` or `setProjectSection`, and drops a closed pane's badge and status, so set them on every pass and send `null` only to clear a pane that stays open. Open diffs with `openDiffWhile`, and repaint on `onGitChange` (Wings 0.1.50) rather than a fast git poll.
- Cache network lookups per repo and branch, not per pane. Local `git` takes about 25 ms and a network CLI 0.7 to 2 s. The dev log prints `[plugin] <id> <program> took N ms` for every call.
- Poll slowly: local checks every 15 s, network every 60 s, and only while Wings is focused: use `pollWhileFocused(tick, ms)` from `wings-plugin-sdk/poll` (or `wings.onFocus`), which ticks when focus returns once `ms` has passed. Offer a Refresh action instead of polling faster. Plugins that use it need `"minWingsVersion": "0.1.17"`.
- Every action handler returns a promise. The button shows a spinner until it settles.
- Never make a click wait on the network. Open the viewer with `openDiff` and no `patch`, then fill it with `updateDiff`. Prefetch heavy data after the status loads.
- `exec` has no shell and no stdin. Pass arguments as an array, use program names rather than paths, and never `sh -c`. That keeps it working on macOS, Windows and Linux.
- Plugin data is untrusted. A new field on `Badge` or `DiffComment` goes in the type and in the matching `clean*` function in `apps/desktop/src/lib/plugins.ts`, with a size bound.
- Don't use deprecated APIs or icons. Check the installed `.d.ts` for `@deprecated` before using a lucide icon.

## Adding a capability to the API

A new `wings.*` call touches five places. Change them together:

1. `apps/desktop/public/plugin-sdk.js`: the call and its JSDoc.
2. `packages/plugin-sdk/wings.d.ts`: the type and its doc comment, which plugin authors read.
3. `apps/desktop/src/lib/plugins.ts`: a case in `handle()` that validates params.
4. `apps/desktop/src-tauri/src/lib.rs`: the Tauri command, registered in `generate_handler!`.
5. `apps/desktop/src-tauri/src/plugins.rs`: the permission check, with a test. `pr_tracker_calls_pass_its_own_manifest` lists PR tracker's calls, so add new ones there.

Then update `plugins/README.md`.

## Check it

```sh
cd plugins && pnpm check
cd apps/desktop && pnpm exec tsc --noEmit && pnpm build
cd src-tauri && cargo clippy && cargo test
```

To check a plugin's output against real data before trying it in the app, run its built `dist/main.js` in Node with a stub `wings` that calls the real CLIs. The bundle hides its functions, so drive it through the listeners it registers, or bundle a small entry that imports the module you want to call:

```js
// harness.mjs: node harness.mjs plugins/<id>/dist/main.js (or a bundle of an entry that sets globalThis.t)
import vm from "node:vm";
import { readFileSync } from "node:fs";
import { execFile } from "node:child_process";

const exec = (program, args, { cwd } = {}) =>
  new Promise((resolve) =>
    execFile(program, args, { cwd, maxBuffer: 64 << 20 }, (err, stdout, stderr) =>
      resolve({ code: err ? (err.code ?? 1) : 0, stdout, stderr })));
let onPanes;
// The SDK's poll helper and channels need focused, onFocus, broadcast and onBroadcast, so they are here. A call this stub
// lacks fails with "wings.<name> is not a function": add a stub for it (onTranscript, onTool, onGitChange, ports...).
const wings = { focused: true, onFocus: () => () => {}, onPanes: (fn) => (onPanes = fn), onAction() {}, onBroadcast: () => () => {}, broadcast: async () => {}, exec, transcript: async () => [], setBadge: async (paneId, badge) => console.log(paneId, badge), openUrl: async () => {} };
const ctx = vm.createContext({ wings, console, setTimeout, clearTimeout, queueMicrotask, setInterval: () => 0, clearInterval, URL, JSON, Promise, Date });
vm.runInContext(readFileSync(process.argv[2], "utf8"), ctx);
// A pane in a real checkout; the badges it sets print as they come.
await onPanes([{ paneId: "p1", cwd: process.cwd(), command: "zsh", project: "demo", session: null, focused: true }]);
```

Wings is a GUI the user is often using. Don't click through it yourself. Ask them to check what the harness can't show.
