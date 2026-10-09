---
name: wings-plugin
description: Creates or changes a Wings plugin, a folder with `wings-plugin.json` and a `main.js` that runs sandboxed inside the Wings desktop app and draws badges, actions and diffs on terminal panes. Use when asked to build a plugin, add a badge, action or diff to one, or add a new capability to the plugin API.
---

# Wings plugins

The API reference is `plugins/README.md`: manifest, permissions, every `wings.*` call, and the badge and comment shapes. Read it first. `plugins/pr-tracker/` is the worked example.

## Where the pieces live

| Piece | File |
|---|---|
| SDK loaded into the plugin frame (`window.wings`) | `apps/desktop/public/plugin-sdk.js` |
| Host: runs frames, relays calls, cleans badges and comments | `apps/desktop/src/lib/plugins.ts` |
| Permission checks, `exec`, transcript reads | `apps/desktop/src-tauri/src/plugins.rs` |
| Tauri commands `plugin_exec`, `plugin_transcript`, `plugin_open_url` | `apps/desktop/src-tauri/src/lib.rs` |
| Live transcript entries for `onTranscript`, read each detection tick | `apps/desktop/src-tauri/src/tail.rs`, `tail_transcripts` in `lib.rs` |
| Claude Code's status line data for `wings.statusline()`, sent by `wings statusline` | `apps/desktop/src-tauri/src/statusline.rs`, `statusline` in `cli.rs` and `control.rs` |
| Badge pill and card | `apps/desktop/src/components/pane-badge.tsx` |
| Diff viewer | `apps/desktop/src/components/diff-viewer.tsx` |

## Make a plugin

1. Create `plugins/<id>/wings-plugin.json`. The `id` is lowercase letters, digits and dashes, and `api` is `1`.
2. Ask for the least you need. `exec` lists commands with their subcommand, like `gh pr view`, `transcript` lists Claude Code entry types, and `openUrl` lists https prefixes. Pass a folder with `{ cwd }`, not `git -C`, because the subcommand has to come first.
3. Write `main.js` against `window.wings` only. The frame is hidden, so all UI goes through badges and the diff viewer.
4. Declare what it adds in `contributes`: `ui` (`badges`, `diff`), `sidebars` (its own pages in the right sidebar), `panels` (outside web pages in a popover) and any `mcpTools`. The host refuses UI calls the manifest didn't declare. Each MCP tool gets an `inputSchema` and a `wings.onTool(name, handler)` in `main`. Claude sees it as `<id>__<name>` once the user clicks Connect under Plugins, Claude Code. For an API, declare `permissions.fetch`, keep the token with `wings.secrets.set` and send it with `wings.fetch(url, { bearer })`, so plugin code never holds it after setup. `~/Desktop/wings-cyclops` is a worked sidebar plugin.
5. Run `pnpm tauri dev` in `apps/desktop`. Debug builds load the repo's `plugins/` folder at startup, always on, so restart Wings after editing one. A plugin in its own repo starts from `templates/wings-plugin/` and installs through the Plugins manager as a `.wings-plugin` file or a GitHub link.

## Rules that keep plugins fast and safe

- Work from `wings.onPanes`. Each pane has a live `cwd` and a `session` while Claude runs in it.
- Follow a session with `wings.onTranscript` rather than polling `wings.transcript`, which rereads the whole file.
- Cache network lookups per repo and branch, not per pane. Local `git` takes about 25 ms and a network CLI 0.7 to 2 s. The dev log prints `[plugin] <id> <program> took N ms` for every call.
- Poll slowly: local checks every 15 s, network every 60 s. Offer a Refresh action instead of polling faster.
- Every action handler returns a promise. The button shows a spinner until it settles.
- Never make a click wait on the network. Open the viewer with `openDiff` and no `patch`, then fill it with `updateDiff`. Prefetch heavy data after the status loads.
- `exec` has no shell and no stdin. Pass arguments as an array, use program names rather than paths, and never `sh -c`. That keeps it working on macOS, Windows and Linux.
- Plugin data is untrusted. A new field on `Badge` or `DiffComment` goes in the type and in the matching `clean*` function in `apps/desktop/src/lib/plugins.ts`, with a size bound.
- Don't use deprecated APIs or icons. Check the installed `.d.ts` for `@deprecated` before using a lucide icon.

## Adding a capability to the API

A new `wings.*` call touches four places. Change them together:

1. `apps/desktop/public/plugin-sdk.js`: the call and its JSDoc.
2. `apps/desktop/src/lib/plugins.ts`: a case in `handle()` that validates params.
3. `apps/desktop/src-tauri/src/lib.rs`: the Tauri command, registered in `generate_handler!`.
4. `apps/desktop/src-tauri/src/plugins.rs`: the permission check, with a test. `pr_tracker_calls_pass_its_own_manifest` lists the example plugin's calls, so add new ones there.

Then update `plugins/README.md`.

## Check it

```sh
node --check plugins/<id>/main.js
cd apps/desktop && pnpm exec tsc --noEmit && pnpm build
cd src-tauri && cargo clippy && cargo test
```

To check a plugin's output against real data before trying it in the app, run its code in Node with a stub `wings` that calls the real CLIs:

```js
// harness.mjs: node harness.mjs plugins/<id>/main.js
import vm from "node:vm";
import { readFileSync } from "node:fs";
import { execFile } from "node:child_process";

const exec = (program, args, { cwd } = {}) =>
  new Promise((resolve) =>
    execFile(program, args, { cwd, maxBuffer: 64 << 20 }, (err, stdout, stderr) =>
      resolve({ code: err ? (err.code ?? 1) : 0, stdout, stderr })));
const wings = { onPanes() {}, onAction() {}, exec, transcript: async () => [], setBadge: async () => {}, openUrl: async () => {} };
const ctx = vm.createContext({ wings, console, setInterval: () => 0, URL, JSON, Promise, Date });
// Expose the top-level functions you want to call.
vm.runInContext(readFileSync(process.argv[2], "utf8") + "\n;globalThis.t = { githubDiff };", ctx);
console.log(await ctx.t.githubDiff({ url: "https://github.com/o/r/pull/1", number: 1, repo: "o/r" }));
```

Wings is a GUI the user is often using. Don't click through it yourself. Ask them to check what the harness can't show.
