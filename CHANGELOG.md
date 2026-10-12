# Changelog

What changed in each Wings release, newest first.

## Unreleased

## 0.1.15 · 2026-10-12

- Less memory: a terminal hidden for 10 minutes keeps its history in a compact copy and frees the rest, and restores it when you come back. Plugin pages you closed or haven't looked at for a while unload too, and reload when you open them.
- opencode panes show whether the agent is working, waiting for you or idle, from opencode's own event stream.
- ⌥← and ⌥→ move a word, ⌘← and ⌘→ go to the start and end of the line, and ⌘⌫ deletes to the start, as in Terminal.
- Toasts have a close button, and Clear all when there are several.
- Plugins can add their own page to Wings Settings, and open a view as a tab next to the terminals.
- Plugin pages use the new `wings-plugin-ui` package, with the app's own components and styling. `wings-plugin-sdk` 0.6.0 keeps the types, the page frame and the build. Changes 0.2.2, HUD 0.3.3 and Session detail 0.4.2 move to it and need this Wings.

## 0.1.14 · 2026-10-12

- Pin plugin buttons to the title bar from the … menu, under Keep in title bar. Pinned buttons always show.
- Plugin pages can stop listening: every `wings.on*` call returns a function that removes the listener.
- The built-in plugins are TypeScript, with React sidebars, built with the new `wings-plugin-sdk` package. They behave as before, with these fixes:
  - PR tracker 0.6.2: a missing gh, az or glab shows the Install badge wherever it's used, an expired gh token shows Sign in, and a closed pane's PR stops being checked.
  - Session detail 0.4.1: busy sessions no longer empty the sidebar, an open row updates when its tool finishes, and rows of different sessions no longer share their open state.
  - Changes 0.2.1: a search you left before it ran no longer loads in the background.
  - Work item 0.2.1: an error shows on its own work item instead of on every pane in the org, and Open only shows once there's a link.
  - Dev servers 0.2.1: only ports with a button can be opened.
- Wings and `wings-plugin-sdk` are MIT-licensed.

## 0.1.13 · 2026-10-12

- With the sidebar closed, point at the window's left edge to show it over the panes and switch projects or agents without opening it.

## 0.1.12 · 2026-10-12

- Plugins installed from a folder of a GitHub repo, like Wings' own, install and update from the default branch, so a plugin update no longer waits for a Wings release. A plugin can set `minWingsVersion`, and an older Wings won't install it.
- PR tracker 0.6.1 shows an Azure PR's merge strategy policy in its own Merge row instead of counting it as a failed check.

## 0.1.11 · 2026-10-12

- Changes 0.2.0 adds commit history: a History tab in its sidebar with search and Load more, a commit graph window with every branch's lanes, and each commit's diff in the diff viewer.
- Dev servers credits a server to the pane that started it, also when one pane's shell runs inside another's.

## 0.1.10 · 2026-10-12

- Plugins can show a small popover when you point at their title-bar button. Cyclops uses it for today's timers and entries.
- New Changes plugin: a badge with each pane's changed lines, and a sidebar with the changed files as a tree. Both open the diff viewer, for uncommitted changes or the whole branch.
- When a plugin needs Azure CLI, GitHub CLI or GitLab CLI and it isn't installed, Wings offers to install it with Homebrew.
- Plugin update checks use your `gh` sign-in when you have one, and ask GitHub less, so they no longer hit its rate limit.

## 0.1.9 · 2026-10-12

- Dev servers 0.2.0 opens servers in your browser only.
- The Plugins section has more room, and the Claude Code and terminal command cards no longer squeeze their text.
- Settings is a little bigger.

## 0.1.8 · 2026-10-12

- Plugins live in Settings, with a list, each plugin's details and its README.
- Wings checks for plugin updates in the background, and Check for updates runs it now. Each plugin with an update gets an Update button.
- See what's new before you update: the update toast and Check for Updates show the release notes.
- Resume a past session in the current tab, from its menu in Past sessions.
- Wings' own plugins install and update from a GitHub link, like `github.com/josippapez/Wings/plugins/hud`.
- The usage badge always shows the 5-hour and weekly limits, also at 0% and before the first reply.
- Plugin badges in a split pane show without hovering.

## 0.1.7 · 2026-10-12

- Settings open in their own window (⌘,), with sections for text, cursor, history and keyboard.
- Menus are sized to their items.
- The terminal shows one more row.

## 0.1.6 · 2026-10-11

- Plugins can open a page in a modal or a window of its own.
- Terminal settings: font family, line height, cursor, scrollback, copy on select and minimum contrast.
- Find in the terminal with ⌘F.
- Terminals show inline images (SIXEL and iTerm2).
- Programs running in a terminal can write to the clipboard.
- ⌘J jumps to the next agent that needs you, and working agents show how long they have been running.
- Clicking a notification takes you to its pane.
- Claude can list your sessions, read a pane, type a command into a pane (never run it) and send you a notification.
- New Dev servers plugin that shows the ports your terminals listen on.

## 0.1.5 · 2026-10-09

- Plugin entries in the title bar's overflow menu stay on one line and show their label in its color.

## 0.1.4 · 2026-10-09

- Plugin commands can list the only flags they allow.
- Long permission entries wrap in the approval dialog.
- New work item plugin that shows the work item of the current branch.
- The pull request tracker shows why checks failed, and Claude can read the failing checks.

## 0.1.3 · 2026-10-09

- Check for Updates… in the Wings menu.
- Reply to review comments from the diff viewer.
- Azure pull requests no longer show the reviewer policy as a running check.

## 0.1.2 · 2026-10-09

- Reply to review comments from the diff viewer.
- Azure pull requests no longer show the reviewer policy as a running check.

## 0.1.1 · 2026-10-09

- Test release for the in-app updater.
