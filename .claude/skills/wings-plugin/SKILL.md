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
| Badge pill and card | `apps/desktop/src/components/pane-badge.tsx` |
| Diff viewer | `apps/desktop/src/components/diff-viewer.tsx` |

## Make a plugin

1. Create `plugins/<id>/wings-plugin.json`. The `id` is lowercase letters, digits and dashes, and `api` is `1`.
2. Ask for the least you need. `exec` lists programs by name, `transcript` lists Claude Code entry types, and `openUrl` lists https prefixes.
3. Write `main.js` against `window.wings` only. The frame is hidden, so all UI goes through badges and the diff viewer.
4. Run `pnpm tauri dev` in `apps/desktop`. Debug builds load the repo's `plugins/` folder. Plugins load at startup, so restart Wings after editing one.

## Rules that keep plugins fast and safe

- Work from `wings.onPanes`. Each pane has a live `cwd` and a `session` while Claude runs in it.
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
4. `apps/desktop/src-tauri/src/plugins.rs`: the permission check, with a test.

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
