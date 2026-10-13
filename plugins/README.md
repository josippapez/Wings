# Wings plugins

A plugin is a folder with a `wings-plugin.json` manifest and a JavaScript file. Wings loads it into a sandboxed frame, and the plugin talks to Wings only through the `wings` object. Anything that touches your machine is checked in Rust against the permissions in the manifest.

`changes/` shows each pane's changed files as a badge and a sidebar tree, the repo's commit history, and a commit graph in a window of its own (`views`), all opening Wings' diff viewer.

`dev-servers/` puts a `localhost:<port>` badge on a pane while a program in it listens on a local port, and opens it in the browser or in a Wings window (`views`).

`worktrees/` lists the git worktrees of your open projects in a sidebar, with what each one is (merged into the default branch, upstream gone, uncommitted changes, locked, folder missing, open in a pane). It removes the ones you pick, or all merged and clean ones, after you confirm, with `git worktree remove` and never `--force`, prunes records of worktrees whose folder is gone, and can delete the branch with `git branch -d`.

`work-item/` shows the Azure Boards work item named in each pane's branch, and moves it to its next state. It also checks Azure DevOps every 2 minutes (5, 10 or 30 under Settings, or off), even while Wings is in the background, for the projects of your panes' Azure remotes and of work items it has shown: a state change on a work item assigned to or created by you, a comment mentioning you on any recently changed work item or on a pull request you created or review, new comments on your pull requests, a reviewer's vote on them, one being completed or abandoned, and you being added as a reviewer. It only reads (`az boards query` and GET calls of `az devops invoke`), sends a desktop notification per event (one summary past three), and lists them with links in its Updates sidebar, since a notification can't be clicked. The first check of a project only records where it is, so old activity doesn't arrive as a flood. A comment mentions you when it carries your Azure DevOps identity id, `data-vss-mention="version:2.0,<id>"` in a work item comment and `@<id>` in a pull request comment. Each check looks into the 20 newest changed work items and the 20 newest pull requests of a project, so a very busy project can miss a mention. A work item opens as a tab with its description, comments and linked pull requests, the Updates list also opens as a tab with read and unread, and the work items assigned to you in the current sprint show under each Azure DevOps project in the sidebar.

`notes/` is retired: Project info moved into Wings as each project's Project page, which agents reach with Wings' own `project_info_*` tools. Version 0.3.0 only points there.

`tasks/` keeps a task list per project: a short list in a sidebar, a board with To do, Doing and Done columns in a tab, and the open tasks under the project in the sidebar. Claude lists, adds and updates tasks with `tasks__list`, `tasks__add`, `tasks__update` and `tasks__done`, for the project of the pane it came from.

`pr-tracker/` is the example: it shows the pull request for each pane's branch as a badge with live status, checks and why they failed, the diff and review comments. It lists each project's pull requests in the sidebar (yours and the ones waiting for your review), and in a tab or a Project page block every open one with All open, Mine, Review requested and Drafts chips (the choice is kept with `setTabState` / `setBlockState`). The PR tab's Checks view lists every check or policy grouped by state, with required or optional, kind, duration, an Open link and, for a failure with a log, the end of it fetched when the row opens. It opens one as a tab to read, reply to, approve and send to an agent. A tab opened with no pull request (from its title bar button) lists the project's open ones to pick from. "View in diff" on a thread opens the viewer at its file and line. It works with GitHub (github.com and GitHub Enterprise Server hosts `gh` is signed in to), GitLab (gitlab.com and self-managed hosts `glab` is signed in to) and Azure DevOps through their CLIs, and with Bitbucket Cloud and Codeberg through an API token saved on its Settings page. Replies work on every host: in the review thread on GitHub (github.com and Enterprise), GitLab and Azure DevOps, under the comment on Bitbucket Cloud, and on Codeberg as a new comment that quotes the one it answers, since the Gitea API has no threaded replies. The Approve button (confirmed first) works on GitHub, GitLab, Bitbucket Cloud and Codeberg; Azure DevOps has a Vote menu (Approve, Approve with suggestions, Wait for author, Reject, Reset vote) that runs `az repos pr set-vote`, which finds your reviewer identity itself. Bitbucket needs the `write:pullrequest:bitbucket` token scope, Codeberg `write:issue` to reply and `write:repository` to approve.

## Installing

Open Plugins (the puzzle icon in the title bar) and paste a GitHub repo link, choose a `.wings-plugin` file, or drop one on the window. A new plugin starts off. Turning it on shows what it can do and what it adds, and approves exactly that. An update that asks for more turns it off until you approve again.

From a GitHub link, Wings installs the newest release's `.wings-plugin` file, or the repo itself if there are no releases, and offers updates when a new release is out. A link to a folder in a repo of several plugins, `github.com/owner/name/plugins/<id>`, installs that folder from the default branch and offers an update when the plugin's own version there changes, so pushing a version bump ships it. An update that needs a newer Wings, by its `minWingsVersion`, isn't offered. Public repos need no sign-in. For a private repo Wings uses your `gh` sign-in, so `gh auth login` as someone who can see it. You can also share a private plugin as a `.wings-plugin` file.

The plugins in this folder install the same way:

| Plugin | Link |
| --- | --- |
| Changes | `github.com/josippapez/Wings/plugins/changes` |
| Dev servers | `github.com/josippapez/Wings/plugins/dev-servers` |
| HUD | `github.com/josippapez/Wings/plugins/hud` |
| PR tracker | `github.com/josippapez/Wings/plugins/pr-tracker` |
| Session detail | `github.com/josippapez/Wings/plugins/session-detail` |
| Tasks | `github.com/josippapez/Wings/plugins/tasks` |
| Work item | `github.com/josippapez/Wings/plugins/work-item` |
| Worktrees | `github.com/josippapez/Wings/plugins/worktrees` |

From a terminal or a script, use the `wings plugin` command while Wings is open:

```sh
wings plugin install ~/Downloads/cyclops.wings-plugin   # or github.com/owner/name
wings plugin list            # --json for scripts
wings plugin enable cyclops  # also update, disable, remove
```

A plugin installed this way still waits for you to approve its access in Wings, which opens the approval. On first start Wings offers to add the `wings` command to `~/.local/bin`, and the Plugins sheet offers it later. With no arguments the command opens Wings rather than starting a second copy.

Plugins start and stop as you turn them on and off, with no restart. Installed plugins live in `<app data>/plugins/<id>/` (on macOS `~/Library/Application Support/dev.wings.app/plugins/`). Dev builds also load this repo's `plugins/` folder at startup, always on.

## Packaging

A `.wings-plugin` file is a zip of the plugin folder, with `wings-plugin.json` at its root or one folder down. A plugin written in TypeScript makes one with `wings-plugin pack` from `wings-plugin-sdk`, which builds `src/` and packs the built files with the manifest, HTML and CSS. A plain JavaScript plugin can zip its git repo:

```sh
npx wings-plugin pack                                          # TypeScript: writes <id>.wings-plugin
git archive --format=zip --output my-plugin.wings-plugin HEAD  # plain JavaScript
```

`templates/wings-plugin/` is a starter repo with the SDK and a workflow that builds and attaches the package to a GitHub release when you push a version tag, so the repo holds only source. Wings installs a repo's release package. A TypeScript repo with no release yet can't be installed from its link, since its source has no built files.

## TypeScript and React

Wings runs plain JavaScript: `main` and each page's `<script>`. To write a plugin in TypeScript, or a page in React, bundle it into those files before you package it and ship the built files. The `wings-plugin-sdk` npm package does that with `wings-plugin build`, and has the types and a React hook. `templates/wings-plugin/` is a plugin set up with it.

Every `wings.on*` call returns a function that removes the listener, so a React effect can return it as its cleanup.

The plugins in this folder do that. Their code is in `<id>/src/`, and every file at the top of `src/` builds to the file of the same name next to `wings-plugin.json`, so `src/main.ts` becomes `main.js` and `src/sidebar.tsx` becomes `sidebar.js`. Folders inside `src/` hold the modules. The built files are committed, because Wings installs a plugin folder straight from `main`.

```sh
cd plugins
pnpm install
pnpm build   # writes each plugin's built files
pnpm check   # type-checks, and fails if a committed built file is out of date
```

Pages use `wings-plugin-ui`: a page calls `showSidebar(<Page />)` (or `showView`, `showPopover`, `showTab`, `showSettings`) and builds from its components, and Wings' `plugin-ui.css` styles them, so the build writes the page's HTML and a Wings update restyles every plugin. A page can still bring its own components and CSS (`import "./sidebar.css"`). See its [README](../packages/plugin-ui/README.md) for the components, the `w-*` class contract and the `--wings-*` variables.

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
| `exec` | Running these commands, with no shell and no stdin, 30 s timeout by default. Each entry is a program and the subcommand the arguments must start with, so `gh pr view` allows `gh pr view <url> --json title`. A bare program name allows any arguments. An entry can end with the only flags it allows, like `az boards work-item update [--id --state]`, which refuses `--title` or any other flag |
| `post` | POSTing to these API paths through a signed-in CLI with `wings.post`. Each entry is `gh`, `glab` or `az` and a path where `*` stands for one segment, like `gh repos/*/*/pulls/*/comments/*/replies`. `az` paths are full `https://` URLs. An entry may end with ` {field=a\|b}`, which pins that body field to one of those values, like `gh repos/*/*/pulls/*/reviews {event=APPROVE}` (an approval that can't be turned into a "request changes"); a request whose field has another value, or none, is refused. `exec` itself refuses every flag that writes, like `-X` or `--method` |
| `transcript` | Reading these Claude Code transcript entry types. `attachment:<kind>`, like `attachment:model`, allows one kind of attachment entry: attachments include whole files and hook output, so ask for the kinds you need rather than `attachment` |
| `openUrl` | Opening URLs that start with these prefixes: https, or http to this computer only (`localhost`, `127.0.0.1`, `[::1]`, without credentials), like `http://localhost:` |
| `fetch` | Calling https URLs through `wings.fetch`. A bare entry is a prefix for any method, like an API's base URL ending in `/`. `GET <prefix>` allows only GET and HEAD. `POST <url>` allows one write: `*` stands for one path segment, there is no query, and the body must be a JSON object (or absent, when nothing is pinned) up to 64 KB, like `POST https://api.example.com/2.0/repos/*/*/pulls/*/comments`. A POST entry may end with ` {field=a\|b}` to pin body fields, like `POST https://codeberg.org/api/v1/repos/*/*/pulls/*/reviews {event=APPROVED}`. Wings refuses `PUT`, `PATCH` and `DELETE` on a `GET` or `POST` entry. Needs Wings 0.1.27 |
| `panes` | Opening terminals with `wings.openPane` and moving focus with `wings.focusPane`. Each entry is a command it may start in the new pane, matched like `exec`, so `npm run dev` allows `npm run dev --port=3000`. `[]` allows plain shells only. Leave it out and the plugin can do neither |
| `notify` | `true` to show desktop notifications with `wings.notify` |
| `ports` | `true` to see, with `wings.ports`, which local TCP ports the programs in your terminals listen on, with each program's name, pid and start time. Only ports that `localhost` reaches, in panes Wings started |
| `statusline` | `true` to read what Claude Code tells its status line with `wings.statusline`: your usage limits and each session's context and cache. See [Status line](#status-line) |

`icon`, optional, is an SVG file in the plugin folder, like `"icon.svg"`. The Plugins list shows it instead of the puzzle piece, and a sidebar, view or panel that leaves out its own `icon` uses it on its title bar button, in the overflow menu and on its tab. Wings draws it as a one-colour mask in the current text colour, so use shapes (`fill` or `stroke`, `currentColor` is fine) and expect colours and gradients to come out as a plain silhouette. It must be a relative path to an `.svg` file inside the plugin folder (no `..`, no absolute path, no link out of it), up to 32 KB, with `svg` as its root element, or Wings refuses to load the plugin. It is only ever drawn as an image, never inserted into a page, so scripts, `on*` attributes and links to other files in it do nothing. A sidebar's, view's or panel's `icon` is still one of the built-in names; it can't point to a file. Older Wings ignores the top-level `icon` and keeps the puzzle piece, so it needs no `minWingsVersion`. A sidebar or view that leaves out `icon` is refused by Wings 0.1.19 and older, so set `"minWingsVersion": "0.1.20"` when you do that. `wings-plugin pack` ships every file outside `src/`, `icon.svg` included. Several pages of one plugin show the same icon: give all but one a built-in `icon`.

`minWingsVersion`, optional, is the oldest Wings that can run the plugin, like `"0.1.13"`. Set it when the plugin uses something an older Wings doesn't have. An older Wings refuses to install it and doesn't offer it as an update.

`contributes` lists what the plugin adds, which the manager shows next to it:

| Field | Means |
|---|---|
| `ui` | `badges` for pane header badges, `diff` for the diff viewer. Wings refuses `setBadge` and `openDiff` without them |
| `mcpTools` | `{ name, description, inputSchema? }` tools the plugin offers Claude through the Wings MCP server, handled with `wings.onTool`. Names are lowercase letters, digits and `_`, and Claude sees them as `<plugin id>__<name>`. `inputSchema` is a JSON Schema object for the arguments, `{ "type": "object" }` by default |
| `sidebars` | Up to 3 `{ id, title, icon?, page }`: the plugin's own HTML `page`, shown in the right sidebar from a title bar button. It runs sandboxed like the plugin, with the same `wings` object, and Wings loads `plugin-ui.css` first so plain buttons, inputs and selects look native. Several open sidebars show as tabs, and each keeps running while hidden. A sidebar may add `popover: { page, width, height }`, a smaller page shown under its button when you point at it or focus it, 240 to 480 wide and 160 to 640 high; see [Popovers](#popovers) |
| `views` | Up to 3 `{ id, title, icon?, page, opens?, width?, height?, newTab?, multiple? }`: pages like `sidebars` that can also open in a modal over Wings, in a window of their own or as a tab. `opens` lists where, from `sidebar`, `modal`, `window` and `tab`, and the title bar button opens it in the first; it's `["sidebar"]` by default. `width` and `height` size the modal (560 by 480 by default) and the window when it first opens; Wings remembers a window's size and position after that. The sidebar and the modal offer to move a page that can open as a window into one. A page gets the same `wings` object wherever it opens, and a modal's page stops when the modal closes. `newTab: true` lists a view that opens as a tab in the + menu and the command palette (see Tabs). `multiple: true` lets it open several tabs per project. Both need `tab` in `opens`, or Wings refuses the plugin. Ids are unique across `sidebars` and `views`. Disabling or removing the plugin, or closing Wings, closes its windows |
| `projectSections` | Up to 8 `{ id, title, icon?, all? }`: lists the plugin can show under each project in the sidebar, filled with `wings.setProjectSection`. `id` is like a view id, unique in the plugin, `title` is up to 40 characters, and `icon` is one of the built-in names, or left out to use the plugin's icon. `all` names one of the plugin's views that opens as a tab and lists everything: the section header's All button opens it in that project (Wings 0.1.29; older Wings ignore it). See Project sections. Needs Wings 0.1.23 |
| `blocks` | Up to 8 `{ id, title, page, icon?, minHeight? }`: pages the user can place on a project's Project page, each in a frame of its own. `id` is like a view id, unique among the blocks, `title` is up to 40 characters, `page` is an HTML file in the plugin, which a view may share, `icon` is one of the built-in names or left out for the plugin's icon, and `minHeight` is 80 to 800 CSS pixels (default 160). See Blocks. Needs Wings 0.1.24 |
| `settings` | `{ page }`: the plugin's own HTML page in Wings Settings, listed under Plugin settings by the plugin's name. It runs sandboxed like a sidebar, with the same `wings` object, fills the settings area and stops when you leave it or close Settings. Keep what it saves in `wings.storage`. See [Settings page](#settings-page) |
| `panels` | Up to 3 `{ id, title, icon?, url, width?, height? }` web pages, each opened in a popover from a title bar button. `icon` is one of `clock`, `globe`, `calendar`, `chart`, `list`, `git-compare`, `git-graph`, `git-branch`, `folder-tree`, `message`, `scroll`, `kanban`, `gauge`, `activity`, `timer`, `server`, `terminal`, `todo` or `puzzle` (Wings 0.1.20 and newer for all but the first five), and `url` must be https. The page runs as a normal website with no access to Wings, and keeps its cookies, so a sign-in sticks |

`id` is lowercase letters, digits and dashes. `api` must be `1`.

A command runs as you, with that CLI's own sign-in, so `gh api` can read anything your GitHub account can. Only declare commands you need, because that list is what the user approves. On top of it, Wings refuses known flags that run other commands, touch files outside the repo, change data on the server or print a token: `git -c`, `--upload-pack`, `--output`, `--no-index`, `gh`/`glab` `-X`, `--method`, `-f`, `-F`, `--input`, `--show-token`, and for `az` `--method`, `--http-method`, the file flags (`--in-file`, `--out-file`, `--output-file`, `--file`, `--destination`, `--source` and their short forms) and `@path` values, which az reads as files. That list can't cover every subcommand of every CLI, so treat an `exec` entry as trusting the plugin with that command.

A `panes` command is typed into the new pane's shell, the way Wings resumes a session, so you're back at a prompt when it exits. It runs as you in a terminal you can see, the same trust as `exec`, and the same flags are refused. Since a shell reads it, every word of an entry and of a command may only use letters, digits and `-_./:=@%+,`: no spaces inside a word, quotes, `$`, globs or operators. Words past the declared entry come from the plugin and are single-quoted. A `cwd` has to be a folder inside one of your Wings projects once `..` and symlinks are resolved, so neither can lead out of it.

Plugins don't load on Windows yet. WebView2 gives child frames the app's IPC bridge, so each plugin needs its own webview there first.

## API

| Call | Does |
|---|---|
| `wings.onPanes(fn)` | Called with every terminal pane on each change, including a change of focus: `{ paneId, cwd, command, project, projectId, session, focused }[]`. `project` is the project's name and `projectId` its id, the same as `wings.project().id` (Wings 0.1.22). `cwd` follows `cd`. `focused` is true on the focused pane of the tab on screen. `session` is `{ sessionId, name, state, startedAt }` while Claude runs in the pane, else `null`. `startedAt` is when that `claude` started, in ms, since a resumed session's transcript also holds what came before |
| `wings.onFocus(fn)` | Called with `{ focused }` once with the current state, then on every change: `focused` is true while any Wings window (the main one or a view window) is in front, false while you're in another app or Wings is minimized or hidden. A short move between two Wings windows doesn't count as a change. Needs no permission. `wings.focused` reads the last state (true until Wings sends it, which is right after the frame is ready). Needs Wings 0.1.17: set `"minWingsVersion": "0.1.17"`. See Pausing polls |
| `wings.onTool(name, fn)` | Runs one of your `mcpTools` when Claude calls it. `fn(input, { paneId })` gets Claude's arguments and the Wings pane Claude runs in (or `null`), and returns a string or a JSON value. A thrown error goes back to Claude as a failed call. Register it in `main`, since sidebar pages don't get tool calls |
| `wings.onAction(fn)` | Called with `{ paneId, actionId }` when a badge action is clicked. Return a promise: the button shows a spinner until it settles (up to 5 min), and a thrown error is shown in the card |
| `wings.exec(program, args, { cwd, timeoutMs, onOutput })` | Resolves `{ code, stdout, stderr }`. `timeoutMs` is 30 s by default, 5 min at most. `onOutput(line)` gets each stdout and stderr line while the program runs. A program that prints more than 8 MB to stdout or stderr is killed and the call rejects with `Output is larger than 8 MB`, not a truncated result: ask for less (`git diff` a few files at a time) |
| `wings.transcript(sessionId, types, { last? })` | Resolves the matching transcript entries, oldest first. `last` keeps only the newest that many (up to 1000), which is much faster on a long session |
| `wings.statusline()` | Resolves `{ rateLimits, sessions }`, what Claude Code last told `wings statusline`. `rateLimits` is `{ fiveHour, sevenDay, at }`, each window `{ usedPercentage, resetsAt }` with `resetsAt` in epoch seconds, or `null` until a session has reported them (only Pro and Max plans get them). `sessions` has, by session id, `{ model, contextWindowSize, usedPercentage, totalInputTokens, totalOutputTokens, currentUsage, promptCache, at }`, Claude Code's own numbers from the last response. `at` is when Wings got it, in ms |
| `wings.onTranscript(fn)` | Called with `{ sessionId, paneId, entry }` for each transcript entry Claude Code writes while it runs in a pane, when the entry's `type` is in `permissions.transcript`. Only entries written after the plugin started, or after the session started in the pane: read earlier ones with `wings.transcript`. Wings looks for new entries twice a second and skips entries over 256 KB, which are large tool results |
| `wings.setBadge(paneId, badge)` | Shows a badge in the pane header; `null` removes it |
| `wings.openUrl(url)` | Opens the URL in the browser |
| `wings.ports()` | Needs `permissions.ports`. Resolves `{ paneId, pid, name, port, startedAt }[]`: the local TCP ports listening in each pane's shell and everything it started, sorted by pane and port. `startedAt` is in ms. Wings reads the process tree and runs `lsof` with fixed arguments, about 20 ms a call (macOS and Linux with `lsof`; rejects where it is missing), so poll every 15 s at most, and only while focused |
| `wings.openPane({ command?, cwd?, placement? })` | Opens a terminal and resolves `{ paneId }` once it's in `onPanes`. `placement` is `tab` (the default), or `right` or `down` for a split beside the focused pane, which has to be in the project on screen. Without `cwd` it opens at the root of the project on screen. The new pane gets the keyboard |
| `wings.focusPane(paneId)` | Switches to an open pane's project and tab and moves the keyboard to it |
| `wings.installTool(program)` | Asks Wings to install a CLI the plugin runs, after `exec` rejected with `<program> is not installed or not on PATH`. See [Required tools](#required-tools). Resolves `true` once the install started or it's already there, `false` if you cancelled |
| `wings.notify({ title, body? })` | A desktop notification, with your plugin's name before the title. The title is cut at 64 characters and the body at 256. Up to 3 a minute per plugin; more are refused with how long to wait. Clicking it does nothing |
| `wings.openDiff({ title, subtitle, cwd?, patch?, comments?, focus? })` | Opens the diff viewer and resolves `{ id }`. Without `patch` it opens in a loading state. `focus` is `{ path, line?, side?: "old" \| "new" }`: once the patch is shown the viewer opens that file, scrolls to the comment thread on that line and highlights the line for a moment (`side` defaults to `new`). It works when the patch arrives later through `updateDiff`, and a path or line the patch doesn't have just opens the diff as usual. Needs Wings 0.1.27. `cwd` is the repo or folder the diff is of: the viewer's review comments (written by the user on any line, sent to an agent with Send to agent) offer the agent working there first |
| `wings.updateDiff(id, { patch, comments, focus? } \| { error })` | Fills in or fails a viewer opened with `openDiff`; ignored once it's closed. A new `focus` is acted on once, so an update with only `comments` leaves where the user is |
| `wings.onReply(handler)` | A reply typed in the diff viewer under a comment with `canReply`: `handler({ diffId, replyTo, body })`, where `replyTo` is the thread's first comment. Post it, then add it with `updateDiff`. The reply box waits for the returned promise, and a thrown error shows under it with the text kept |
| `wings.post(program, url, fields, { host })` | POSTs `fields` (string or whole-number values, up to 64 KB) as JSON to a `permissions.post` path, as you: `gh api` (pass `host` for a GitHub Enterprise Server; github.com is the default) or `glab api` against `host`, or `az rest` for Azure DevOps. Wings builds the command itself. Resolves `{ code, stdout, stderr }` |
| `wings.fetch(url, { method, headers, body, bearer })` | An HTTP call made by Wings to a `permissions.fetch` URL. `bearer` names a secret sent as `Authorization: Bearer <secret>`. Resolves `{ status, contentType, body }` for any status and doesn't follow redirects. The URL must already be in normal form, and its path may only use letters, digits and `-_.~/`. Headers are limited to `Accept`, `Accept-Language`, `Content-Type`, `Cache-Control`, `If-None-Match` and `If-Modified-Since` |
| `wings.secrets.set(name, value)`, `.delete(name)`, `.has(name)` | Secrets like API tokens, kept in the system keychain under the plugin's name. They can be sent by `fetch` but not read back, and go when the plugin is removed or replaced by one from elsewhere |
| `wings.storage.get(key)`, `.set(key, value)`, `.delete(key)`, `.keys()` | The plugin's own storage for settings and state, since `localStorage` throws in plugin pages. Values are JSON (what `JSON.stringify` keeps), and `get` resolves `null` for a key that isn't set. Keys are 1 to 128 letters, digits and `- _ . : /`. Everything together is limited to 1 MB, and a `set` that would go over is refused. See [Storage](#storage) |
| `wings.openView(id, { as?, args? })` | Opens one of your `sidebars` or `views`: `as` is `sidebar`, `modal`, `window` or `tab`, one of the view's `opens`, and the first by default. A window that's already open comes to the front, and so does the view's tab in the project on screen. Only one modal shows at a time. `args` is JSON, up to 16 KB, and only with `as: "tab"`: the tab's page reads it with `wings.viewArgs()`. A tab that's already open with different `args` takes the new ones, drops the state it saved with `setTabState` (it belonged to the old args) and reloads, and with the same args or none just comes to the front. A view with `multiple` always opens a new tab. Needs Wings 0.1.22 |
| `wings.viewArgs()` | What `openView` passed as `args` when it opened this tab, or `null`. Synchronous, and a copy. Needs Wings 0.1.22 |
| `wings.placement()` | Where this page shows: `"sidebar"`, `"tab"`, `"modal"`, `"window"` or `"block"` (on a Project page), so a page can show more detail when it has room. `null` in the main script. A sidebar's popover reports `"sidebar"` and the settings page `"modal"`. Synchronous. Needs Wings 0.1.22 |
| `wings.project()` | Resolves `{ id, name, path, branch }` of the project the page is about, or `null` when none is open. A tab page's project is the one the tab belongs to and never changes. Any other page gets the project on screen. Needs Wings 0.1.22 |
| `wings.onProject(fn)` | Calls `fn` with the current project, then each time it changes: the project on screen switched (never for a tab page), or its branch or name changed. Returns a function that removes the listener. Needs Wings 0.1.22 |
| `wings.projects()` | Resolves every project, `{ id, name, path, branch }[]`, in sidebar order. Worktrees are projects too. Works in the main script and in pages. Needs Wings 0.1.23 |
| `wings.onProjects(fn)` | Calls `fn` with every project, then each time one is added, removed or renamed, or a branch changes. Returns a function that removes the listener. Needs Wings 0.1.23 |
| `wings.setProjectSection(projectId, sectionId, rows)` | Shows `rows` as a section under that project in the sidebar, or removes it with `null`. `sectionId` is one the manifest declares in `contributes.projectSections`. A row is `{ id, label, detail?, badge?: { text, tone? }, open: { view, args? } }`. Rejects for an unknown project, an undeclared section, or an `open.view` that isn't one of the plugin's views with `tab` in `opens`. See Project sections. Needs Wings 0.1.23 |
| `wings.tabState()` | In a tab page: resolves the state it last saved with `setTabState`, or `null`. Outside a tab it resolves `null`. Needs Wings 0.1.22 |
| `wings.setTabState(state)` | In a tab page: saves JSON state, up to 64 KB, on the tab. Wings saves it with the tab and restores it on the next start and when the page loads again after being unloaded. Rejects outside a tab and when the state is too big. Needs Wings 0.1.22 |
| `wings.blockState()` | In a block: the state it last saved with `setBlockState`, or `null`. `null` outside a block. See Blocks. Needs Wings 0.1.24 |
| `wings.setBlockState(state)` | In a block: saves JSON state, up to 16 KB, with the Project page, like which board or filter the block shows. Rejects outside a block and when the state is too big. Needs Wings 0.1.24 |
| `wings.setHeight(px)` | In a block: asks for a height in CSS pixels. Wings rounds it, clamps it between the block's `minHeight` and 1200 and applies it without reloading the page. Asking for the height already asked for does nothing, and calls in one tick send the last. Throws outside a block. Needs Wings 0.1.24 |
| `wings.closeView(id)` | Closes a sidebar or view wherever it's open |
| `wings.openSettings()` | Opens Wings Settings at the plugin's settings page. Refused for a plugin without `contributes.settings` |
| `wings.setSidebarLabel(sidebarId, label, { rows, tone })` | Takes a sidebar or view id. Up to 80 characters next to its title bar button, like a running timer; `null` clears it. Wings shows it as a green pill, cut with an ellipsis when it doesn't fit, or an amber or red one when `tone` is `"warning"` or `"danger"`. `rows` are up to 6 `{ label, value }` shown when you point at it |
| `wings.broadcast(message)`, `wings.onBroadcast(fn)` | Sends a JSON value, up to 64 KB, to the plugin's other pages: its main script and any open sidebars, popovers, modals and windows. Use it so a page refreshes when a tool call changed something |

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

Wings adds tools of its own to that server, with or without plugins:

| Tool | What it does |
| --- | --- |
| `search_history`, `read_session` | Find and read past Claude Code sessions. |
| `list_sessions` | The open panes: id, project, folder, program and, for Claude, its state (working, blocked, done or idle), what it waits for, session id and name. |
| `read_pane(pane_id, lines?)` | The last up to 200 lines (32 KB) of a pane's output as plain text. Marked as data, since a pane can print anything. |
| `open_pane(cwd?, command?)` | A new tab in a project (`cwd` has to be inside one you added). The command is typed but never run: you read it and press Enter. It can't hold a newline or other control character. |
| `notify(title, body?)` | A desktop notification titled "Claude: …", up to 3 a minute across all sessions. |

Mind what these allow. Every Claude Code session you start on this computer can call them, so one session can read what another shows, which can include secrets and prompts. `open_pane` only types a command, so nothing runs outside Claude Code's own permission checks for Bash; Claude Code still asks before a call to an MCP tool unless you allowed it. The socket is readable by your user only (`0600`) and there is no network access.

The `wings-plugin-sdk` package has types for all of this.

## Pausing polls

Wings is in the background most of the day, and a timer that runs `git`, `gh` or `lsof` then spends the user's battery on a badge nobody sees. Stop timers while `wings.focused` is false and check once when it turns true. `pollWhileFocused(tick, ms)` from `wings-plugin-sdk/poll` does this: it runs `tick` every `ms` while Wings is in front, and once at once when you come back. Keep event-driven work (`onPanes`, `onTranscript`, actions, tools) running. `onFocus` also tells you when to call `wings.notify`: send it only when `wings.focused` is false, since a user who is looking doesn't need a notification.

## Required tools

A plugin needs the GitHub CLI (`gh`), the GitLab CLI (`glab`) or the Azure CLI (`az`) when its `exec` or `post` entries name it. Its page in Plugins lists them under Needs, each Installed or not. A missing one has a button: on a Mac with Homebrew, Wings asks, then opens a tab in the project on screen and runs the vendor's own Homebrew command there (`brew install gh`, `brew install glab`, `brew update && brew install azure-cli`), so you see its output and can answer it. Without Homebrew, or off macOS, it links to the vendor's install page and to brew.sh. When the command is done, Wings checks again, says so, and restarts the plugins that run it.

`exec` rejects with `<program> is not installed or not on PATH` for a missing program. A plugin can then show a badge with an Install action that calls `wings.installTool(program)`, like `pr-tracker` and `work-item` do. The commands come from Wings' own table, so a plugin only names the program: one in its manifest and in that table, and Wings always asks first.

## Views

A view is one of your pages that can open somewhere other than the sidebar. This one opens in a modal from its title bar button, and can move to a window of its own:

```json
"contributes": {
  "views": [{ "id": "board", "title": "Board", "icon": "list", "page": "board.html", "opens": ["modal", "window"], "width": 720, "height": 520 }]
}
```

Any of the plugin's pages can open it, and keep it current while it's open in any of those places:

```js
await wings.openView("board", { as: "window" });
wings.broadcast({ refreshed: Date.now() }); // board.html hears it with wings.onBroadcast
```

A window shows the same sandboxed page as the sidebar would. Its calls go to Wings' main window and are checked there against your manifest, exactly like the sidebar's, so a window can't do more than the plugin could.

With `tab` in `opens`, the view opens as a tab in the project on screen, next to its terminal tabs, and fills the pane area. Each project has at most one tab per view, and opening it again shows that tab. The page keeps running while another tab shows, and stops when you close the tab, with its close button, a middle click or ⌘W, or when the plugin calls `wings.closeView`. Wings saves the tab with the others and opens it again on the next start, unless the plugin is gone or the view no longer lists `tab`.

Wings 0.1.14 and older refuse a plugin with `tab` in `opens`, so set `"minWingsVersion": "0.1.15"` to use it.

A view with `"newTab": true` also shows in the + menu, a chevron next to the + button, with the plugin's icon, and in the command palette as "New tab: <title>". Under Settings, General, "New tab button opens" picks what the + button itself opens: a terminal (the default) or one of these pages. `"multiple": true` makes an open of the view, from + or `openView`, a new tab instead of showing the project's existing one, except that an open with `args` equal to those of a tab of it already in the project shows that tab. Give each tab its own `wings.setTabState` so they don't share. `newTab`, `multiple`, `args`, `project`, `placement` and tab state need Wings 0.1.22, so set `"minWingsVersion": "0.1.22"` when you use them.

## Project sections

A plugin can list things under each project in the sidebar, like its open tasks or pull requests. Declare the section in the manifest, then fill it per project from the main script:

```json
"contributes": { "projectSections": [{ "id": "open", "title": "Open tasks", "icon": "todo" }] }
```

```ts
wings.onProjects((projects) => {
  for (const p of projects) {
    wings.setProjectSection(p.id, "open", [
      { id: "t1", label: "Fix the login", detail: "due Friday", badge: { text: "High", tone: "warning" }, open: { view: "board", args: { task: "t1" } } },
    ]);
  }
});
```

A section shows under its project after the project's row, with the plugin's icon, its title and the row count, only while it has rows. Clicking its header folds it shut, and Wings remembers that per project and section. Clicking a row opens `open.view` as a tab in that row's project, switching to the project, with `args` for `viewArgs()` and following the same rules as `openView`: a tab already open with other args takes the new ones and reloads, and a view with `multiple` opens a new tab unless a tab of it in that project already has equal `args`. `open.view` must be one of the plugin's views with `tab` in `opens`, or the call rejects. Wings cuts text for you (label and detail 120 characters, badge 24, ids 64), keeps the first 50 rows, leaves out a row with no `id` or `label` or an `id` already used, and rejects `args` over 16 KB. A badge `tone` is one of the badge tones. Sections are kept in memory, not saved: they go when the plugin is turned off or removed, so fill them again on start, and send only when rows change. They need Wings 0.1.23, so set `"minWingsVersion": "0.1.23"`.

## Blocks

A block is a plugin page the user places on a project's Project page, between its sections, like a sprint board or the open pull requests. Each block runs in its own sandboxed frame, with the same `wings` object as a tab page.

```json
"contributes": {
  "views": [{ "id": "board", "title": "Board", "page": "board.html", "opens": ["tab"] }],
  "blocks": [{ "id": "board", "title": "Sprint board", "page": "board.html", "minHeight": 200 }]
}
```

A block's `page` can be the page of a view, so the same HTML shows as a tab and as a block; read `wings.placement()` to draw a compact version when it is `"block"`. Declaring a block adds nothing to the sidebar or the + menu: it shows only in the Project page's "+ Add" menu, for plugins that are on and approved.

In a block:

- `wings.project()` is the Project page's project, and never changes.
- `wings.blockState()` and `wings.setBlockState(state)` keep a small JSON value (up to 16 KB) per block, like the filter it shows. Two blocks of the same page have their own state. Wings saves it with the Project page.
- `wings.setHeight(px)` sizes the block to its content, within `minHeight` and 1200. A page that measures itself can call it from a `ResizeObserver`; Wings applies the height without reloading the page, and the call returns at once.
- Outside a block, `blockState()` gives `null`, and `setBlockState` and `setHeight` fail with a message saying so.

Blocks cost a frame each, so Wings loads one only when it scrolls into view, unloads it after 5 minutes out of view (a page that saved its state with `setBlockState` comes back as it was) and shows at most 8 at once on a Project page. While a block is out of view, or its project's tab is hidden, its `wings.focused` is false from the first line of the page (not only after the first `onFocus`) and `onFocus` reports each change, so `pollWhileFocused` pauses its polling and checks once when the block is back. Gate polling on `wings.focused`, not `document.visibilityState`: a frame scrolled out of view or in a hidden tab still reports `"visible"`, because the browser only reports the state of the whole Wings window. A block page draws with `showView` from `wings-plugin-ui`; `showBlock` isn't needed. Don't poll from a block's page faster than you would from a sidebar.

Blocks, `"block"` and the three calls need Wings 0.1.24, so set `"minWingsVersion": "0.1.24"`. Older Wings ignore `contributes.blocks`.

## Popovers

A sidebar can have a quick look: a second page shown under its title bar button after you point at it for a moment, or when the button gets the keyboard. Clicking the button still opens the sidebar, and the popover doesn't show while that sidebar is open. It closes shortly after the pointer leaves both, or with Escape on the button.

```json
"contributes": {
  "sidebars": [{ "id": "timer", "title": "Timer", "icon": "clock", "page": "sidebar.html", "popover": { "page": "popover.html", "width": 340, "height": 420 } }]
}
```

The page gets the same `wings` object, storage and broadcasts as the sidebar. It loads the first time it shows and keeps running while closed, so it opens at once after that and its timers keep going. Send the user to the full view with `wings.openView("timer")`.

## Settings page

A plugin can have a page in Wings Settings instead of a settings form in its sidebar:

```json
"contributes": {
  "settings": { "page": "settings.html" }
}
```

Any of the plugin's pages, or its main script, can send the user there, and hear what changed with `wings.onBroadcast`:

```js
await wings.openSettings();
// settings.html, after saving:
await wings.storage.set("interval", 60);
wings.broadcast({ settingsChanged: true });
```

Wings 0.1.14 and older ignore `contributes.settings` and have no `wings.openSettings`. Set `"minWingsVersion": "0.1.15"` when the plugin needs them, or check `typeof wings.openSettings === "function"` first.

## Status line

Claude Code sends its status line command a JSON status after each reply: the 5-hour and weekly usage limits, the context window and how much of it is used, and the prompt cache. It's the documented way to get the usage limits, with no sign-in token involved. Wings keeps the newest status for plugins with `permissions.statusline`, and only the fields listed under `wings.statusline()`.

When Wings starts, it adds itself to Claude Code: its MCP server, and `wings statusline --pass` in front of the `statusLine` command in `~/.claude/settings.json`. Your own status line stays in that command and runs after Wings on the same input, so it still shows, and the rest of the file is left as it was. Disconnect in Plugins puts your status line back and stops Wings adding itself again. Dev builds don't connect on their own, since they'd point Claude Code at their own binary.

## Storage

`wings.storage` needs no permission: it's the plugin's own data, and no other plugin can read it. Wings keeps it in one file per plugin, `<app data>/plugin-storage/<id>.<scope>.json` (on macOS `~/Library/Application Support/dev.wings.app/plugin-storage/`). Each change writes a new file and renames it into place, so a crash never leaves half a file. The main script and the sidebar pages share it, and writes they make at the same time all land.

It follows the same rule as secrets. An update from the same GitHub repo keeps it. Removing the plugin deletes it, and so does installing a plugin with the same id from a file or another repo, which gets a new `<scope>`, so a different package can't read what the old one stored. Keep tokens in `wings.secrets`, not here.
