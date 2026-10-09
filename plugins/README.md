# Wings plugins

A plugin is a folder with a `wings-plugin.json` manifest and a JavaScript file. Wings loads it into a sandboxed frame, and the plugin talks to Wings only through the `wings` object. Anything that touches your machine is checked in Rust against the permissions in the manifest.

`pr-tracker/` is the example: it shows the pull request for each pane's current branch as a badge on the pane, for GitHub (`gh`), GitLab (`glab`) and Azure DevOps (`az` and `git`). When a check fails, the badge shows which step failed and its error, and the `failing_checks` tool hands Claude the end of each failed log. When a CLI isn't signed in, the badge has a Sign in button that runs the CLI's own browser login.

## Installing

Open Plugins (the puzzle icon in the title bar) and paste a GitHub repo link, choose a `.wings-plugin` file, or drop one on the window. A new plugin starts off. Turning it on shows what it can do and what it adds, and approves exactly that. An update that asks for more turns it off until you approve again.

From a GitHub link, Wings installs the newest release's `.wings-plugin` file, or the repo itself if there are no releases, and offers updates when a new release is out. Public repos need no sign-in. For a private repo Wings uses your `gh` sign-in, so `gh auth login` as someone who can see it. You can also share a private plugin as a `.wings-plugin` file.

From a terminal or a script, use the `wings plugin` command while Wings is open:

```sh
wings plugin install ~/Downloads/cyclops.wings-plugin   # or github.com/owner/name
wings plugin list            # --json for scripts
wings plugin enable cyclops  # also update, disable, remove
```

A plugin installed this way still waits for you to approve its access in Wings, which opens the approval. On first start Wings offers to add the `wings` command to `~/.local/bin`, and the Plugins sheet offers it later. With no arguments the command opens Wings rather than starting a second copy.

Plugins start and stop as you turn them on and off, with no restart. Installed plugins live in `<app data>/plugins/<id>/` (on macOS `~/Library/Application Support/dev.wings.app/plugins/`). Dev builds also load this repo's `plugins/` folder at startup, always on.

## Packaging

A `.wings-plugin` file is a zip of the plugin folder, with `wings-plugin.json` at its root or one folder down. From a git repo:

```sh
git archive --format=zip --output my-plugin.wings-plugin HEAD
```

`templates/wings-plugin/` is a starter repo with SDK types and a workflow that attaches the package to a GitHub release when you push a version tag.

## Manifest

```json
{
  "id": "pr-tracker",
  "name": "PR tracker",
  "version": "0.1.0",
  "description": "Shows the pull request for each pane's branch.",
  "api": 1,
  "main": "main.js",
  "permissions": {
    "exec": [
      "git branch --show-current",
      "gh pr list",
      "gh pr view"
    ],
    "transcript": [
      "pr-link"
    ],
    "openUrl": [
      "https://github.com/"
    ]
  },
  "contributes": {
    "ui": [
      "badges",
      "diff"
    ]
  }
}
```

| Permission | Allows |
|---|---|
| `exec` | Running these commands, with no shell and no stdin, 30 s timeout by default. Each entry is a program and the subcommand the arguments must start with, so `gh pr view` allows `gh pr view <url> --json title`. A bare program name allows any arguments |
| `post` | POSTing to these API paths through a signed-in CLI with `wings.post`. Each entry is `gh`, `glab` or `az` and a path where `*` stands for one segment, like `gh repos/*/*/pulls/*/comments/*/replies`. `az` paths are full `https://` URLs. `exec` itself refuses every flag that writes, like `-X` or `--method` |
| `transcript` | Reading these Claude Code transcript entry types. `attachment:<kind>`, like `attachment:model`, allows one kind of attachment entry: attachments include whole files and hook output, so ask for the kinds you need rather than `attachment` |
| `openUrl` | Opening https URLs that start with these prefixes |
| `fetch` | Calling https URLs that start with these prefixes through `wings.fetch`, like an API's base URL ending in `/` |
| `panes` | Opening terminals with `wings.openPane` and moving focus with `wings.focusPane`. Each entry is a command it may start in the new pane, matched like `exec`, so `npm run dev` allows `npm run dev --port=3000`. `[]` allows plain shells only. Leave it out and the plugin can do neither |
| `notify` | `true` to show desktop notifications with `wings.notify` |
| `statusline` | `true` to read what Claude Code tells its status line with `wings.statusline`: your usage limits and each session's context and cache. See [Status line](#status-line) |

`contributes` lists what the plugin adds, which the manager shows next to it:

| Field | Means |
|---|---|
| `ui` | `badges` for pane header badges, `diff` for the diff viewer. Wings refuses `setBadge` and `openDiff` without them |
| `mcpTools` | `{ name, description, inputSchema? }` tools the plugin offers Claude through the Wings MCP server, handled with `wings.onTool`. Names are lowercase letters, digits and `_`, and Claude sees them as `<plugin id>__<name>`. `inputSchema` is a JSON Schema object for the arguments, `{ "type": "object" }` by default |
| `sidebars` | Up to 3 `{ id, title, icon, page }`: the plugin's own HTML `page`, shown in the right sidebar from a title bar button. It runs sandboxed like the plugin, with the same `wings` object, and Wings loads `plugin-ui.css` first so plain buttons, inputs and selects look native. Several open sidebars show as tabs, and each keeps running while hidden |
| `panels` | Up to 3 `{ id, title, icon, url, width?, height? }` web pages, each opened in a popover from a title bar button. `icon` is `clock`, `globe`, `calendar`, `chart` or `list`, and `url` must be https. The page runs as a normal website with no access to Wings, and keeps its cookies, so a sign-in sticks |

`id` is lowercase letters, digits and dashes. `api` must be `1`.

A command runs as you, with that CLI's own sign-in, so `gh api` can read anything your GitHub account can. Only declare commands you need, because that list is what the user approves. On top of it, Wings refuses known flags that run other commands, touch files outside the repo, change data on the server or print a token: `git -c`, `--upload-pack`, `--output`, `--no-index`, `gh`/`glab` `-X`, `--method`, `-f`, `-F`, `--input`, `--show-token`, and for `az` `--method`, `--http-method`, the file flags (`--in-file`, `--out-file`, `--output-file`, `--file`, `--destination`, `--source` and their short forms) and `@path` values, which az reads as files. That list can't cover every subcommand of every CLI, so treat an `exec` entry as trusting the plugin with that command.

A `panes` command is typed into the new pane's shell, the way Wings resumes a session, so you're back at a prompt when it exits. It runs as you in a terminal you can see, the same trust as `exec`, and the same flags are refused. Since a shell reads it, every word of an entry and of a command may only use letters, digits and `-_./:=@%+,`: no spaces inside a word, quotes, `$`, globs or operators. Words past the declared entry come from the plugin and are single-quoted. A `cwd` has to be a folder inside one of your Wings projects once `..` and symlinks are resolved, so neither can lead out of it.

Plugins don't load on Windows yet. WebView2 gives child frames the app's IPC bridge, so each plugin needs its own webview there first.

## API

| Call | Does |
|---|---|
| `wings.onPanes(fn)` | Called with every terminal pane on each change: `{ paneId, cwd, command, project, session }[]`. `cwd` follows `cd`. `session` is `{ sessionId, name, state, startedAt }` while Claude runs in the pane, else `null`. `startedAt` is when that `claude` started, in ms, since a resumed session's transcript also holds what came before |
| `wings.onTool(name, fn)` | Runs one of your `mcpTools` when Claude calls it. `fn(input, { paneId })` gets Claude's arguments and the Wings pane Claude runs in (or `null`), and returns a string or a JSON value. A thrown error goes back to Claude as a failed call. Register it in `main`, since sidebar pages don't get tool calls |
| `wings.onAction(fn)` | Called with `{ paneId, actionId }` when a badge action is clicked. Return a promise: the button shows a spinner until it settles (up to 5 min), and a thrown error is shown in the card |
| `wings.exec(program, args, { cwd, timeoutMs, onOutput })` | Resolves `{ code, stdout, stderr }`. `timeoutMs` is 30 s by default, 5 min at most. `onOutput(line)` gets each stdout and stderr line while the program runs |
| `wings.transcript(sessionId, types, { last? })` | Resolves the matching transcript entries, oldest first. `last` keeps only the newest that many (up to 1000), which is much faster on a long session |
| `wings.statusline()` | Resolves `{ rateLimits, sessions }`, what Claude Code last told `wings statusline`. `rateLimits` is `{ fiveHour, sevenDay, at }`, each window `{ usedPercentage, resetsAt }` with `resetsAt` in epoch seconds, or `null` until a session has reported them (only Pro and Max plans get them). `sessions` has, by session id, `{ model, contextWindowSize, usedPercentage, totalInputTokens, totalOutputTokens, currentUsage, promptCache, at }`, Claude Code's own numbers from the last response. `at` is when Wings got it, in ms |
| `wings.onTranscript(fn)` | Called with `{ sessionId, paneId, entry }` for each transcript entry Claude Code writes while it runs in a pane, when the entry's `type` is in `permissions.transcript`. Only entries written after the plugin started, or after the session started in the pane: read earlier ones with `wings.transcript`. Wings looks for new entries twice a second and skips entries over 256 KB, which are large tool results |
| `wings.setBadge(paneId, badge)` | Shows a badge in the pane header; `null` removes it |
| `wings.openUrl(url)` | Opens the URL in the browser |
| `wings.openPane({ command?, cwd?, placement? })` | Opens a terminal and resolves `{ paneId }` once it's in `onPanes`. `placement` is `tab` (the default), or `right` or `down` for a split beside the focused pane, which has to be in the project on screen. Without `cwd` it opens at the root of the project on screen. The new pane gets the keyboard |
| `wings.focusPane(paneId)` | Switches to an open pane's project and tab and moves the keyboard to it |
| `wings.notify({ title, body? })` | A desktop notification, with your plugin's name before the title. The title is cut at 64 characters and the body at 256. Up to 3 a minute per plugin; more are refused with how long to wait. Clicking it does nothing |
| `wings.openDiff({ title, subtitle, patch?, comments? })` | Opens the diff viewer and resolves `{ id }`. Without `patch` it opens in a loading state |
| `wings.updateDiff(id, { patch, comments } \| { error })` | Fills in or fails a viewer opened with `openDiff`; ignored once it's closed |
| `wings.onReply(handler)` | A reply typed in the diff viewer under a comment with `canReply`: `handler({ diffId, replyTo, body })`, where `replyTo` is the thread's first comment. Post it, then add it with `updateDiff`. The reply box waits for the returned promise, and a thrown error shows under it with the text kept |
| `wings.post(program, url, fields, { host })` | POSTs `fields` (string or whole-number values, up to 64 KB) as JSON to a `permissions.post` path, as you: `gh api`, `glab api` against `host`, or `az rest` for Azure DevOps. Wings builds the command itself. Resolves `{ code, stdout, stderr }` |
| `wings.fetch(url, { method, headers, body, bearer })` | An HTTP call made by Wings to a `permissions.fetch` URL. `bearer` names a secret sent as `Authorization: Bearer <secret>`. Resolves `{ status, contentType, body }` for any status and doesn't follow redirects. The URL must already be in normal form, and its path may only use letters, digits and `-_.~/`. Headers are limited to `Accept`, `Accept-Language`, `Content-Type`, `Cache-Control`, `If-None-Match` and `If-Modified-Since` |
| `wings.secrets.set(name, value)`, `.delete(name)`, `.has(name)` | Secrets like API tokens, kept in the system keychain under the plugin's name. They can be sent by `fetch` but not read back, and go when the plugin is removed or replaced by one from elsewhere |
| `wings.storage.get(key)`, `.set(key, value)`, `.delete(key)`, `.keys()` | The plugin's own storage for settings and state, since `localStorage` throws in plugin pages. Values are JSON (what `JSON.stringify` keeps), and `get` resolves `null` for a key that isn't set. Keys are 1 to 128 letters, digits and `- _ . : /`. Everything together is limited to 1 MB, and a `set` that would go over is refused. See [Storage](#storage) |
| `wings.setSidebarLabel(sidebarId, label, { rows, tone })` | Up to 80 characters next to the sidebar's title bar button, like a running timer; `null` clears it. Wings shows it as a green pill, cut with an ellipsis when it doesn't fit, or an amber or red one when `tone` is `"warning"` or `"danger"`. `rows` are up to 6 `{ label, value }` shown when you point at it |
| `wings.broadcast(message)`, `wings.onBroadcast(fn)` | Sends a JSON value, up to 64 KB, to the plugin's other pages: its main script and any open sidebars. Use it so a sidebar refreshes when a tool call changed something |

A badge is `{ label, tone, icon?, counts?, loading?, title?, subtitle?, rows?, actions? }`:

- `tone` is one of `neutral`, `info`, `success`, `warning`, `danger`, `merged`.
- `icon` is one of `pr-open`, `pr-merged`, `pr-closed`, `pr-draft`.
- `counts` are `{ icon, value }` chips after the label, where `icon` is `changed`, `push` or `pull`. Leave out zeros.
- `rows` are `{ label, value, tone? }`, shown in the card when you click the badge.
- `actions` are `{ id, label, primary? }` buttons in that card. A click calls `onAction`.
- `loading: true` shows a spinner in the pill while the plugin refreshes.

A review comment is `{ id, replyTo, path, line, side, author, body, createdAt, url, canReply }`, where `side` is `additions` or `deletions`, and `canReply` on a thread's first comment shows a reply box under it. `path` is `null` for a comment on the whole pull request, and `line` is `null` for comments on code that has changed since. Both show under Discussion in the viewer.

Wings drops a pane's badges when the pane closes. When several plugins add to the same place, the extra pane badges fold into a "+N" button and title bar buttons past the first three into a menu.

Plugin tools reach Claude through one MCP server, `wings`, which you add once from the Plugins sheet with Connect. It runs `wings --mcp` at user scope, so every Claude Code session gets the tools of the plugins that are on, and the list updates as you turn plugins on and off. The tools only work while Wings is open.

`templates/wings-plugin/wings.d.ts` has types for all of this.

## Status line

Claude Code sends its status line command a JSON status after each reply: the 5-hour and weekly usage limits, the context window and how much of it is used, and the prompt cache. It's the documented way to get the usage limits, with no sign-in token involved. Wings keeps the newest status for plugins with `permissions.statusline`, and only the fields listed under `wings.statusline()`.

When Wings starts, it adds itself to Claude Code: its MCP server, and `wings statusline --pass` in front of the `statusLine` command in `~/.claude/settings.json`. Your own status line stays in that command and runs after Wings on the same input, so it still shows, and the rest of the file is left as it was. Disconnect in Plugins puts your status line back and stops Wings adding itself again. Dev builds don't connect on their own, since they'd point Claude Code at their own binary.

## Storage

`wings.storage` needs no permission: it's the plugin's own data, and no other plugin can read it. Wings keeps it in one file per plugin, `<app data>/plugin-storage/<id>.<scope>.json` (on macOS `~/Library/Application Support/dev.wings.app/plugin-storage/`). Each change writes a new file and renames it into place, so a crash never leaves half a file. The main script and the sidebar pages share it, and writes they make at the same time all land.

It follows the same rule as secrets. An update from the same GitHub repo keeps it. Removing the plugin deletes it, and so does installing a plugin with the same id from a file or another repo, which gets a new `<scope>`, so a different package can't read what the old one stored. Keep tokens in `wings.secrets`, not here.
