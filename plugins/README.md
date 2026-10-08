# Wings plugins

A plugin is a folder with a `wings-plugin.json` manifest and a JavaScript file. Wings loads it into a sandboxed frame, and the plugin talks to Wings only through the `wings` object. Anything that touches your machine is checked in Rust against the permissions in the manifest.

`pr-tracker/` is the example: it shows the pull request for each pane's current branch as a badge on the pane, for GitHub (`gh`), GitLab (`glab`) and Azure DevOps (`az` and `git`). When a CLI isn't signed in, the badge has a Sign in button that runs the CLI's own browser login.

## Installing

Open Plugins (the puzzle icon in the title bar) and paste a GitHub repo link, choose a `.wings-plugin` file, or drop one on the window. A new plugin starts off. Turning it on shows what it can do and what it adds, and approves exactly that. An update that asks for more turns it off until you approve again.

From a GitHub link, Wings installs the newest release's `.wings-plugin` file, or the repo itself if there are no releases, and offers updates when a new release is out. That works for public repos. Share a private plugin as a `.wings-plugin` file.

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
| `transcript` | Reading these Claude Code transcript entry types |
| `openUrl` | Opening https URLs that start with these prefixes |

`contributes` lists what the plugin adds, which the manager shows next to it:

| Field | Means |
|---|---|
| `ui` | `badges` for pane header badges, `diff` for the diff viewer. Wings refuses `setBadge` and `openDiff` without them |
| `mcpTools` | `{ name, description }` tools the plugin offers Claude through the Wings MCP server. Names are lowercase letters, digits and `_`. Wings doesn't serve them yet |

`id` is lowercase letters, digits and dashes. `api` must be `1`.

A command runs as you, with that CLI's own sign-in, so `gh api` can read anything your GitHub account can. Only declare commands you need, because that list is what the user approves. On top of it, Wings refuses known flags that run other commands, touch files outside the repo, change data on the server or print a token: `git -c`, `--upload-pack`, `--output`, `--no-index`, `gh`/`glab` `-X`, `--method`, `-f`, `-F`, `--input`, `--show-token`, and for `az` `--method`, `--http-method`, the file flags (`--in-file`, `--out-file`, `--output-file`, `--file`, `--destination`) and `@path` values, which az reads as files. That list can't cover every subcommand of every CLI, so treat an `exec` entry as trusting the plugin with that command.

Plugins don't load on Windows yet. WebView2 gives child frames the app's IPC bridge, so each plugin needs its own webview there first.

## API

| Call | Does |
|---|---|
| `wings.onPanes(fn)` | Called with every terminal pane on each change: `{ paneId, cwd, command, project, session }[]`. `cwd` follows `cd`. `session` is `{ sessionId, name, state }` while Claude runs in the pane, else `null` |
| `wings.onAction(fn)` | Called with `{ paneId, actionId }` when a badge action is clicked. Return a promise: the button shows a spinner until it settles (up to 5 min), and a thrown error is shown in the card |
| `wings.exec(program, args, { cwd, timeoutMs, onOutput })` | Resolves `{ code, stdout, stderr }`. `timeoutMs` is 30 s by default, 5 min at most. `onOutput(line)` gets each stdout and stderr line while the program runs |
| `wings.transcript(sessionId, types)` | Resolves the matching transcript entries, oldest first |
| `wings.setBadge(paneId, badge)` | Shows a badge in the pane header; `null` removes it |
| `wings.openUrl(url)` | Opens the URL in the browser |
| `wings.openDiff({ title, subtitle, patch?, comments? })` | Opens the diff viewer and resolves `{ id }`. Without `patch` it opens in a loading state |
| `wings.updateDiff(id, { patch, comments } \| { error })` | Fills in or fails a viewer opened with `openDiff`; ignored once it's closed |

A badge is `{ label, tone, icon?, counts?, loading?, title?, subtitle?, rows?, actions? }`:

- `tone` is one of `neutral`, `info`, `success`, `warning`, `danger`, `merged`.
- `icon` is one of `pr-open`, `pr-merged`, `pr-closed`, `pr-draft`.
- `counts` are `{ icon, value }` chips after the label, where `icon` is `changed`, `push` or `pull`. Leave out zeros.
- `rows` are `{ label, value, tone? }`, shown in the card when you click the badge.
- `actions` are `{ id, label, primary? }` buttons in that card. A click calls `onAction`.
- `loading: true` shows a spinner in the pill while the plugin refreshes.

A review comment is `{ id, replyTo, path, line, side, author, body, createdAt, url }`, where `side` is `additions` or `deletions`. `path` is `null` for a comment on the whole pull request, and `line` is `null` for comments on code that has changed since. Both show under Discussion in the viewer.

Wings drops a pane's badges when the pane closes.
