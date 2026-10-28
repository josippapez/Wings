# Changelog

What changed in each Wings release, newest first.

## 0.1.86 · 2026-10-28

- **The coding agents list loads.** Settings > Agents showed "Command harnesses_status not allowed by ACL" instead of your agents. It lists them now.
- **Update notes read properly.** The update dialog showed `**` and backticks around words. It shows bold text and code now.

## 0.1.85 · 2026-10-28

- **See which coding agents you have.** Settings > Agents lists Claude Code, Codex, Cursor CLI, Grok Build and OpenCode, with each one's version and whether you're signed in. Install puts the vendor's install command in a new tab for you to run, and Sign in starts the login there. Cursor CLI shows as installed only: Wings doesn't check its sign-in yet.

## 0.1.84 · 2026-10-28

- **Diffs open beside your work.** A diff from the PR tab, a pane badge or the session panel opens in a split to the right of that tab, so the PR page or the terminal stays in view. Drag the edge to resize it. The expand button fills the window with it, as before, and the button there docks it back. The diff only shows on the tab you opened it from.

## 0.1.83 · 2026-10-27

- **The running dot shows for scripts too.** A tab running a shell script or a wrapped command, like `( … ) | tee`, looked idle. It shows the green dot now.

## 0.1.82 · 2026-10-27

- **Claude hears when a tab finishes.** When Claude starts a command in a new tab, it gets a `wings wait` command that returns once the command ends, with the end of its output. It no longer has to keep checking the tab.

## 0.1.81 · 2026-10-27

- **Long commands open in a tab.** Claude now starts dev servers, watchers and long builds or test runs in a new Wings tab instead of in the background, so you can watch and stop them. Quick commands still run in place.

## 0.1.80 · 2026-10-27

- **Plugins wait for you in Settings.** Settings, Plugins lists every plugin from the Wings repo that you haven't installed under Available, each with an Install button. When a new one comes out, Wings tells you once.

## 0.1.79 · 2026-10-27

- **Settings opens from the menu again.** Wings > Settings… and Cmd+, did nothing. They open Settings now.
- **Disk usage with no project.** With no project open, the + menu and the command palette offer Disk usage as a tab. The tab closes once you add a project.

## 0.1.78 · 2026-10-23

- **Tabs show what's running.** A terminal tab gets a pulsing green dot after its title while one of its panes runs a program, like a dev server or a build, instead of sitting at the prompt. Hover the tab to see which programs.

## 0.1.77 · 2026-10-23

- **Issues close when their pull request merges.** An issue in progress or in review with a linked GitHub or Azure DevOps pull request moves to Done once the PR is merged, with a note saying so. One with open child issues stays open, and a PR closed without merging only leaves a note. Issues asks to be approved again, since it now reads PR states with `gh` and `az`.
- **Issues in review get settled.** While a session's issue is in review, each message you send reminds the agent to set it Done if you say it works, or back to In progress if you report a problem. Plugins can answer each message with the `UserPromptSubmit` hook.
- **Fewer summaries.** A turn under 15 seconds no longer runs the on-device model for a summary.

## 0.1.76 · 2026-10-23

- **Plugins find gh and az after a slow start.** When your shell took too long to start as Wings opened, as it can right after the Mac starts, Wings missed programs installed with Homebrew until it was restarted. It now tries again, so Try again works.

## 0.1.75 · 2026-10-22

- **No stray text after a restart.** A restored terminal no longer types the answers to the old program's terminal queries into the new shell's prompt.
- **Project sorting opens again.** The sort button in the Projects header crashed the app.
- **Sort issues from the sidebar.** The Issues section under a project has a sort button on its header, with the same orders as its setting. Plugins can offer orders for their own sections with `sorts` and `wings.onSectionSort`.

## 0.1.74 · 2026-10-22

- **Terminals keep their output across restarts.** When Wings quits, each pane saves what it showed, and shows it again above a fresh prompt when Wings starts. Closed panes' output is deleted at the next start.

## 0.1.73 · 2026-10-22

- **Sort, pin and fold projects.** A menu in the Projects header sorts projects by hand, by when they were added, by when you last worked on them, or by name. Pin a project from its right-click menu to keep it on top. Projects you haven't worked on for 3 days, with no agent running, fold into a collapsed Older group at the bottom.
- **Shorter project rows.** Each project in the sidebar takes less height.

## 0.1.72 · 2026-10-22

- **No stray Wings in Claude Code's settings.** A copy of Wings from a build folder that had added itself to Claude Code's status line and hooks is now taken out when Wings connects, instead of being kept beside the app. Once the build folder was cleaned, it left the status line without input and made every session start report a hook error.

## 0.1.71 · 2026-10-22

- **Disk usage counts clones once.** Files that share their blocks as APFS clones count once, the way DaisyDisk counts them, so 78 copies Chrome keeps of itself show as 2.4 GB instead of 118 GB and a scan no longer adds up to more than the disk holds.
- **Scans don't download cloud files.** A scan never makes macOS download what an iCloud Drive or other cloud folder holds.
- **Releases clean up after themselves.** Building a release clears the debug build, and publishing clears the release build, which together had grown to 40 GB.

## 0.1.70 · 2026-10-22

- **Disk usage.** A new plugin shows what takes the space on your disks as a map you click into, like DaisyDisk: your disks, a fast scan of a disk or folder, what is in each folder, the hidden space, and a Collector that moves what you pick to the Trash. Claude can scan with its `disk__*` tools and fill the Collector for you to review.
- **Plugins can scan disks.** `wings.diskUsage`, `wings.diskFind` and `wings.disks`, behind the new `diskUsage` permission, walk folders in Wings on several threads.
- **Worktrees sees every project.** It lists the worktrees of every project in the sidebar, not only those with a terminal open, and Scan this Mac finds repos with worktrees anywhere on your disks.
- **Child issues sit under their epic.** A project's Issues in the sidebar show each epic's or feature's children indented under it.

## 0.1.69 · 2026-10-22

- **Agents can open a tab in any folder.** `open_pane` takes a folder outside your Wings projects and opens it as a tab in the calling pane's project. Plugin panes still stay inside their own project.

## 0.1.68 · 2026-10-21

- **Agents read only the part of a past session they need.** `read_session` takes a query and returns the messages that match, each with the one before and after, instead of paging through the whole session.

## 0.1.67 · 2026-10-21

- **Journal notes land on the right issue.** A turn's changed files and commits go to the issue the agent worked on in that turn, even one it closed or hadn't started, and a task wins over its epic. Needs the journal plugin 0.3.2.
- **The journal stays pointed at the installed app.** A build run from the repo no longer takes over where the journal sends its notes, and the app puts back an entry a build already changed.

## 0.1.66 · 2026-10-20

- **Issues shows who wrote what.** An agent's entries all carry its session's current name, even after Claude Code restarts it under another one. Click the name to go to that session's pane, or to resume it when it isn't running. Resume never starts a running session twice. Entries the journal writes say so.
- **Summaries leave out image markers.** A summary no longer repeats `[Image #4]` from a prompt with a pasted image.

## 0.1.65 · 2026-10-20

- **Running agents show their session title again.** The sidebar showed the name Claude Code makes from the folder, like `wings-ff`, instead of the session's title. The title comes first now, and a generated name is only used when there is neither.

## 0.1.64 · 2026-10-20

- **Better generated names.** An agent Claude Code named after its folder, like `wings-b3`, now gets a name from its work. A short first prompt is read together with the next ones, and a session with almost nothing in it keeps showing its first prompt instead of a guessed name.

## 0.1.63 · 2026-10-20

- **Sessions get names and summaries from a local model.** Sessions with no title get a short name in the sidebar, History and Issues, and each agent gets a one-line summary of its last turn under its name. Settings › Agents picks the model: Apple's on-device model (the default), an Ollama model, or off. Nothing leaves your Mac.
- **The Session sidebar works for OpenCode.** An OpenCode pane shows its model, tokens and cost, what it is doing, the files it edited and its recent tool calls. Plugins read it with `wings.agentActivity`.
- **Only the installed app connects the agents.** A Wings binary started from a build folder no longer writes itself into Claude Code's or OpenCode's config.

## 0.1.62 · 2026-10-20

- **Past OpenCode sessions are in History.** They list next to Claude Code's with the agent's icon, search by what was said in them, and Resume runs `opencode --session`. The first load runs in the background and fills in as it goes. `search_history` and `read_session` cover them too.

## 0.1.61 · 2026-10-20

- **Agents' change journals reach Issues.** `wings journal` is the sink a change journal runs at the end of each turn, and Wings sets itself up as that sink while Claude Code or OpenCode is connected. A sink you set up yourself is left alone.
- **Issues keeps a Files changed list.** Each turn's files land on the session's open issue as one row per file with its newest description, on the issue page and in `tasks__get`, and the turn's commits are linked. The log stays for people and decisions.
- **Session start points at stale work.** Issues mentions open issues unchanged for 3 days or more, and a finished todo whose issue is still open. It never changes a status itself.

## 0.1.60 · 2026-10-20

- **Plugin tools work the same in OpenCode as in Claude Code.** A tool call from an OpenCode pane now carries its session, so `tasks__todo` and the Issues tools record OpenCode sessions instead of failing with "no Claude session yet". Plugins see each pane's session with its `kind` (`claude` or `opencode`).
- **Issues keep sessions from both agents.** Each session on an issue shows its agent and has its own Resume, which runs `claude --resume` or `opencode --session`. Start agent asks which agent to start.
- **Settings › Plugins is less crowded.** Claude Code, OpenCode and the terminal command are one row of chips. Click one for its status, Connect or Disconnect, and its settings.

## 0.1.59 · 2026-10-20

- **OpenCode gets plugin tools too.** Wings adds its MCP server to OpenCode's global config when it starts, the way it does for Claude Code, and reloads OpenCode so running sessions pick it up. Settings › Plugins has an OpenCode row with Connect and Disconnect. Fields you set on the `wings` entry, like `enabled`, are kept, and a config with comments is left alone.

## 0.1.58 · 2026-10-17

- **Errors show up where you'll see them.** A failed Add, section or block action on the Project page is a toast, not a line at the bottom of the page. Resume, fork, agent restart, plugin panels, Copy and comment links say when they fail instead of doing nothing.
- **Removing asks first.** Removing a plugin or a fact asks for a second click. Removing a project that has tabs open asks in a dialog, since its tabs and agents close.
- A plugin page that fails to load says so instead of staying blank.
- Long names in notifications, the sidebar, History and diff threads show in full on hover.
- PR tracker 0.22.1 to 0.22.3: Vote opens a menu under its button, a commit in a push opens its changes, a running GitHub job says its log comes when it finishes, and removing a saved token asks first.
- Issues 0.14.3, Changes 0.6.3, Session detail 0.6.4 and Work item 0.9.3: confirms and errors scroll into view or sit next to what caused them, commit shas copy, and every linked pull request is listed.

## 0.1.57 · 2026-10-17

- PR tracker 0.22.0: open a build check in the Checks tab to see its run. Jobs and steps with their status and time sit beside the step's log, with the CI's colours, error and warning lines, folded groups and line numbers. Works for Azure DevOps, GitHub Actions and GitLab.
- plugin-ui's list and detail layout no longer shows its back button when the page is wide enough for both panes.

## 0.1.56 · 2026-10-16

- **Comments in the diff viewer render as Markdown.** Bold, lists and links work, a `<details>` block is a fold you can open, and `<!-- -->` comments are hidden. A link in a plugin's comment still opens through that plugin's permission.
- PR tracker 0.21.0: a comment left on an older push says "As of push N" on its code, with "Show latest" to see where those lines are now, the way Azure DevOps shows them. Bot comments no longer show their hidden `<!-- -->` state as text.
- Issues 0.14.2: the session start line tells Claude to find or open the issue before anything else, investigating included.
- plugin-ui hides `<!-- -->` comments in Markdown, so changes, hud, notes, session-detail, work-item and worktrees get a patch release.

## 0.1.55 · 2026-10-16

- **Claude can start a command in a new tab.** `open_pane` takes `run`, and Wings presses Enter after the command when "Let Claude run commands in new tabs" is on in Settings, Agents. It's off by default, and then the command waits for you as before. The reply tells Claude whether it ran, so it doesn't wait on a server that never started.

## 0.1.54 · 2026-10-16

- **Plugins can answer Claude Code's hooks.** A plugin lists `SessionStart` or `PreToolUse` in `contributes.hooks` and answers with `wings.onHook`: it can add context when a session starts, and refuse a Bash command with a reason Claude sees. Connect adds `wings hook` to your Claude Code settings after the hooks you already have, and Disconnect takes it out; with Wings closed or slow, Claude is never held up. Needs Wings 0.1.54.
- **Plugins can float a card over a terminal.** `wings.setPaneRail` shows a list, like the agent's todo list, in a corner of a pane. Drag it to another corner, minimise it to a pill, or close it until it changes. It never takes the terminal's focus and moving it doesn't resize the terminal. Needs `rail` in `contributes.ui` and Wings 0.1.54.
- **A page that opens no longer misses the panes.** `wings.onPanes` registered after Wings sent the panes now hears them right away. Before, the Worktrees sidebar could open saying there was no git repo until you switched panes.

## 0.1.53 · 2026-10-16

- The notification sound also plays when the agent in the pane you're looking at needs you or finishes. Settings › Notifications has "Also for the pane on screen" to turn that off.

## 0.1.52 · 2026-10-16

- Notifications play a sound, also while Wings is focused: when an agent finishes or needs you in a pane you're not looking at, for plugin notifications, and for update toasts. Settings › Notifications turns it off and picks one of four tones, with a preview. Several notifications at once play one sound. The macOS notification no longer plays its own sound, so you hear the chosen tone once.
- PR tracker 0.20.1: a code comment's snippet keeps its space while it loads, the lines a pull request added are tinted green, and the push that opened the pull request says so.

## 0.1.51 · 2026-10-16

- Opening and closing plugin pages no longer grows Wings' memory. Each open used to load the page from a new address, so WebKit cached another copy every time, about 0.4 MB per open. A plugin's pages now share one address until the plugin restarts.
- The approval dialog's step count sits beside its title instead of under the close button.
- PR tracker 0.20.0: a notification when someone mentions you or replies in a thread you wrote in, on any pull request in your projects. Turn it on from its Settings page. It only checks while Wings is in the background.
- Plugin SDK 0.18.0: `wings-plugin init` starts a plugin from the template, and `wings-plugin shot` takes screenshots of a plugin's preview fixtures. plugin-ui 0.11.0: bolder section headings, a Loading spinner, Chips that wrap, Tabs aligned to the content and with disabled items, a small Badge, and Board columns that share the width.

## 0.1.50 · 2026-10-16

- **Plugins store more.** Each value is saved in its own file, with 1 MB per value and 50 MB per plugin instead of 1 MB in all. Existing data moves over on first start, and the old file is kept as `.migrated`. Issues 0.13.0 keeps each issue apart, so writing a note no longer rewrites the whole project, and notes can be up to 20,000 characters.
- **Plugins hear git changes.** A commit, checkout or pull updates PR tracker, Work item, Changes and Worktrees within about a second, without polling git every 15 s. Plugins that may run git get it through `wings.onGitChange`.
- **Pages out of view stop polling.** A tab in the background, a closed sidebar or a closed popover is told it's hidden, and a page that comes back reloads only once its interval is up.
- **Blocks come back after their plugin restarts.** Before, a block on a Project page stayed blank after a plugin was reinstalled at the same version.
- **Tool calls carry the caller's project and Claude session.** Plugins get them with each call.
- **Develop plugins without restarting.** `wings-plugin dev` rebuilds on save and reloads the plugin in Wings, and `wings plugin link <folder>` runs a plugin from its folder.
- **Smaller fixes.** Buttons, badges and tabs inside panes no longer animate `visibility`, which could leave them half drawn after a tab switch. The approval dialog's step count no longer sits under its close button.
- **PR tracker 0.19.0.** Main loads each PR's data once and shares it with the pane badge and every tab showing it. A tab can sign in, add a token or install a CLI itself. Only a new PR someone else opens takes up to 60 s to show.
- **Issues 0.13.0.** A failed save keeps the edit open with the reason. The board's delete confirm stays open when the delete fails.
- **Changes 0.6.0, Worktrees 0.4.0, Work item 0.9.0.** They reload on git changes. Their errors show in notice boxes, and times use one format ("2 h ago").
- **Plugin SDK 0.17.0 and plugin-ui 0.10.0.** Plugins share data between their parts with keyed queries (`serve` in main, `useQuery` in pages). New SDK modules cover messages, settings, sessions, git, CLI calls and async helpers. New plugin-ui parts are Icon, Textarea, InlineInput, Confirm and Notice. Plugins build into `dist/`, and the build checks the manifest against the code.

## 0.1.49 · 2026-10-15

- Settings › Plugins › Claude Code has a "Load Wings tools at session start" switch. It sets `alwaysLoad` on the Wings server, so Claude sees plugin tools from the first message instead of only when something tells it to look. It takes effect in new sessions.

## 0.1.48 · 2026-10-15

- Plugins can give Claude a line on when to use their tools (`contributes.mcpInstructions`), which Claude sees at session start before it has loaded any of them. Issues uses it, so Claude files and looks up issues in Wings without being told to.
- Reconnecting Claude Code keeps what you set on the `wings` server yourself, like `alwaysLoad`.
- Issues 0.10.0: search issues by key, title, description and log notes, with `tasks__list` (`query`) and a search box on the board. Pick the view the board opens in (Board, Rows or List) on its Settings page.

## 0.1.47 · 2026-10-15

- The right sidebar shows a tab for each open plugin page again when more than one is open, with a close button on each. With one open it shows just its title.

## 0.1.46 · 2026-10-15

- The Inbox keeps its last 100 notifications across restarts and updates. After a restart, an agent's entry no longer offers Go to pane, since its pane is gone.

## 0.1.45 · 2026-10-15

- Plugin notifications in the Inbox can have up to three buttons, which open one of the plugin's tabs in the notification's project or a link (`wings.notify({ actions, projectId })`).
- PR tracker 0.15.0 and 0.16.0: set an interval on its Settings page to get a notification when someone opens a new pull request in one of your projects, with Open, Open in browser and All pull requests buttons in the Inbox.

## 0.1.44 · 2026-10-15

- The diff viewer has a scrollbar pinned under each code column, so long lines scroll sideways with a mouse, and Shift with the wheel scrolls them too. A Wrap toggle wraps long lines instead, and Wings remembers the choice.
- Issues 0.9.0: delete an issue from its card or row with the trash button or the Delete key. When it has child issues, choose to delete only it or it with all of them.

## 0.1.43 · 2026-10-15

- Settings shows the running Wings version at the foot of its sidebar.

## 0.1.42 · 2026-10-15

- Issues 0.7.0: the board groups issues in rows by their parent epic or feature, like Linear's sub-grouping. Each row has the parent's status, title (click to open it) and how many of its children are done, and folds shut; the issues with no parent come last. Group by parent turns it off, and the tab keeps both choices. Epic and feature cards show their children's progress too.
- Plugin pages can show a board in rows with `BoardLanes`, `BoardLane` and `BoardCell`; `wings-plugin-ui` 0.9.0, every plugin rebuilt.

## 0.1.41 · 2026-10-15

- Issues 0.6.0 keeps issues local to Wings: it no longer links commits or branches by an issue key in git, and its tools tell agents never to put keys in commit messages, branch names or pull requests. Agents link a commit, branch or pull request to an issue themselves with `update`. Issues no longer needs to run git.

## 0.1.40 · 2026-10-14

- The sidebar's Issues section has an All button that opens the board, like Pull requests. Its rows show the issue's status as an icon, the way Linear does: an empty circle to do, a dashed one in progress, a dotted one in review. Plugins can use these icons on section rows (`todo`, `in-progress`, `in-review`, `done`, `canceled`); `wings-plugin-sdk` 0.16.0. Issues 0.5.1.

## 0.1.39 · 2026-10-14

- A plugin page opens in its open tab; a middle-click or Cmd+click on a plugin's title bar button, + menu entry or section row opens a new tab. Plugins can do the same with `openView(id, { as: "tab", newTab: true })`. `wings-plugin-sdk` 0.15.0.
- Issues 0.5.0:
  - Types (task, bug, feature, epic), a parent epic or feature, related issues (shown on both) and blocked by / blocks. An epic or feature lists its child issues with progress.
  - Commits whose message names an issue key, and branches whose name has one, link to the issue on their own.
  - A new description starts from a template: Goal, Scope, Done when, Validation. Empty sections don't show.
  - Clicking issues reuses one issue tab; a middle-click or Cmd+click opens another.

## 0.1.38 · 2026-10-14

- Updating several plugins that ask for new access reviews them one by one: the dialog says "1 of 3" and moves to the next after Turn on or Not now. Before, only the first one was asked about.

## 0.1.37 · 2026-10-14

- Issues 0.4.1: the issue page reads like an issue: a large title you edit in place, the description under it, the properties (status, branch, pull request, commits, sessions) in a column on the right, and the activity below. Start agent opens Claude with a plain sentence as its prompt. The right sidebar is gone: the board and the project's Issues section list the same.
- The plugin menu in the title bar leaves out pages that show one item, like a single issue or work item. Plugins mark them with `button: false`.
- Plugins can pass values with spaces to a new pane with `openPane({ command, args })`, each quoted as one word, like a prompt for Claude. `wings-plugin-sdk` 0.14.0.

## 0.1.36 · 2026-10-14

- Tasks is now Issues (0.4.0). Each issue has a key like `WEB-12`, a status (to do, in progress, in review, done, canceled), a Markdown description, links to its branch, PR and commits, the Claude sessions that worked on it, and an activity log of who did what and when. An issue opens as a tab with Resume (continues its last session) and Start agent. Claude gets `tasks__list`, `get`, `create`, `update` and `log`, and its entries are stamped with its session. Your tasks are moved over on first start.
- PR tracker 0.14.0: the Overview reads like Azure DevOps': every comment in one timeline (bots too, tagged), pushes as cards listing their commits, and comments on code with the file and the lines around them.
- A plugin can't start `claude` with flags its manifest doesn't name, so no plugin can open Claude with `--dangerously-skip-permissions` or its own settings.

## 0.1.35 · 2026-10-14

- Pull requests in the sidebar show a pull request icon (draft, open, merged, closed) in their status colour instead of a dot. Plugins can set `icon` on a section row; `wings-plugin-sdk` 0.13.0.
- PR tracker 0.13.5: an Azure DevOps pull request's conversation leaves out Azure's own notes, like "The reference … was updated", as Azure DevOps does. Comments from bots and build services fold into an "Automated" group under the conversation, and the Overview count is people's comments.

## 0.1.34 · 2026-10-14

- Back and forward: arrow buttons next to the sidebar toggle, your mouse's back and forward buttons (also over plugin pages), and ⌥⌘[ / ⌥⌘]. They go between the projects and tabs you visited.
- HUD 0.5.1 colours the line under Claude panes: green while there's room, amber from 75% (70% for context), red from 90% (85%).
- Comments read like they do on the web: a `<details>` section folds under its summary, and `[KEY]: value` lines that bots leave for tracking are hidden. `wings-plugin-ui` 0.8.1; every plugin is rebuilt with it.

## 0.1.33 · 2026-10-14

- Comments read newest first: the PR tab's conversation (every host), the work item's discussion and the diff viewer's discussion. A thread moves to the top when someone replies; replies inside a thread still read top to bottom. PR tracker 0.13.3, Work item 0.7.1.

## 0.1.32 · 2026-10-14

- A calmer top: Search, Inbox and Past sessions sit at the top of the left sidebar and Settings at its bottom. The project bar shows project › branch with its git counts, and plugin buttons as plain icons with a small status dot and text.
- The right sidebar drops its tab strip: the plugin buttons switch between open pages.
- Plugins can show a status line under a pane's terminal with `wings.setPaneStatus` (`contributes.ui: ["status"]`). `wings-plugin-sdk` 0.12.0.
- HUD 0.5.0 shows your 5-hour and weekly usage, context and failed MCP servers in that line under every Claude pane, always visible. Its pane badge and title bar text are gone; click the line for the HUD sidebar.

## 0.1.31 · 2026-10-13

- A plugin may show up to 20 desktop notifications a minute, up from 3. `wings-plugin-sdk` 0.11.1 says so in its types.
- Work item 0.7.0 drops the Updates feed and its notifications: it only saw pull requests you created or review, so mentions elsewhere were missed.
- Visual polish across plugin pages, each checked in screenshots at two widths:
  - PR tracker 0.13.2: reviewer avatars clear their label, code blocks wrap at narrow widths, check times line up, a titled error state.
  - Work item: sprint cards wrap their titles instead of cutting them.
  - Tasks 0.3.1: the board's columns share the width, errors sit under the toolbar, rows line up.
  - Changes 0.3.2: the list stays on screen when git reports an error, shown above it. History puts branch and tag labels on their own row, and the commit graph lines up at narrow widths.
  - HUD 0.4.1 and Session 0.5.1: tidier sidebar rows.
- `wings-plugin-ui` 0.7.0: a disabled text box matches the app's.

## 0.1.30 · 2026-10-13

- Plugin pages take their colours, radius and font from the app's theme, and their buttons, tabs, chips, selects and menus now match the app's. `wings-plugin-ui` 0.6.0.
- The toasts' Clear all button is a small pill lined up under the stack's right edge.
- Fewer pane badges: Session 0.5.0 drops its model and context badge, and HUD 0.4.0 shows its badge only when the context runs past 70% or an MCP server fails or needs sign-in. The HUD sidebar keeps the full numbers.
- The diff viewer no longer redraws the open file every time the app updates.
- Changes 0.3.1: History keeps the commits "Load more" added when a new commit lands, so the list stays where you were reading.
- Pane badges no longer stay blank until hovered after switching back to a tab.
- PR tracker 0.13.1: a pull request opened from the All list has its back link in the page toolbar, as a breadcrumb.

## 0.1.29 · 2026-10-13

- Big diffs open fast: the diff viewer draws only the lines near the screen and highlights code in the background. A 30,000-line file went from about 4.4 s to 0.1 s in testing. "View in diff" lands on the right line in split and unified view.
- Tabs start at the main pane's left edge and follow the sidebar as you resize it.
- A section in the sidebar can have an All button that opens the plugin's full list. Pull requests use it to open every open PR of the project.
- The plugin approval dialog scrolls when a plugin asks for a lot.
- The Project page and the Tasks pages are redesigned.
- PR tracker 0.13.0: a Checks tab lists every check and policy with its status, failing ones open to the end of their log, and Azure policies say what's missing. The pull request list shows every open PR with All, Mine, Review requested and Drafts filters.
- PR tracker 0.12.0 votes on Azure DevOps pull requests: Approve, Approve with suggestions, Wait for author, Reject or Reset vote, each confirmed first.

## 0.1.28 · 2026-10-13

- A plugin call that a permission pins to one value, like approve only, always goes out as JSON. A plugin could otherwise send it as form data that a server reads as a different value.

## 0.1.27 · 2026-10-13

- Tabs get the whole title bar. The project name, branch, plugin buttons, history and settings move to a slim bar under the tabs.
- "New tab button opens" in Settings offers Terminal, Choose each time, Project page, or any plugin page that declares `newTab`.
- Projects in the sidebar list pull requests, work items and tasks as one-line rows with a status dot. Leading tags like `[Waiting on decision]` are hidden, and long lists show 5 rows, then "Show N more".
- Project page: add an item at the top, search the page with ⌘F, and an option to open new projects on their Project page.
- PR tracker 0.11.0: "View in diff" jumps to the comment's line, replies work on GitHub Enterprise, Bitbucket and Codeberg, and Approve works on GitHub, GitLab, Bitbucket and Codeberg. A pull request tab opened on its own lists the project's pull requests.
- Work item 0.6.0: the work item tab reads like the PR page, with Overview, Links and History, and Updates has filters. The sidebar's work items leave out finished ones and show each under the repo it's linked to.
- Changes 0.3.0 shows big diffs: a file over 1 MB shows as too large instead of the whole diff failing. Every plugin command also returns about 20 ms sooner.
- Plugins can only send to hosts their CLI is signed in to, and a write permission can pin a field so it can't be bypassed with a different-case key.

## 0.1.26 · 2026-10-13

- The changed-files count uses a plain pencil icon. The old file-and-pencil icon looked like two icons on top of each other at that size.

## 0.1.25 · 2026-10-13

- A project row in the sidebar makes room for both of its hover buttons, so they no longer cover the name and the change count.
- PR tracker 0.10.0 shows a pull request like a review page: the description and comments as Markdown with tables, code and emoji, reviewers with their votes, checks with failing ones first, and the conversation as threads with their status. Resolved and bot threads start collapsed.
- wings-plugin-ui 0.4.0: emoji shortcodes like `:white_check_mark:` in `Markdown`, and an `Avatar` component.

## 0.1.24 · 2026-10-13

- Each project has a Project page: a table of facts, Markdown sections with tables, lists and pasted images, and live blocks from plugins. Open it from the project in the sidebar, the + menu or the command palette. What you kept in Project info moves into it the first time it opens.
- Agents read and write the Project page with Wings' own `project_info_*` tools, which need no plugin. They never replace a section you edited.
- Plugin blocks: Tasks adds its board, Work item the current sprint (yours or the whole sprint), and PR tracker the project's open pull requests. A block loads when it scrolls into view and stops checking for updates while it's out of sight.
- PR tracker 0.9.0 also works with GitHub Enterprise Server, self-managed GitLab, Bitbucket Cloud and Codeberg. Bitbucket and Codeberg need an API token, added on its Settings page. When a host isn't signed in, its block and badge offer a Sign in or Add token button.
- The Notes plugin (Project info) is retired. Its 0.3.0 has no tools and only points to the Project page, so agents see one place for project info.
- Plugin SDK 0.10.0 adds `contributes.blocks`, `wings.blockState()`, `wings.setBlockState()` and `wings.setHeight()`.

## 0.1.23 · 2026-10-13

- Projects in the sidebar list what plugins add under them, like open pull requests, tasks and work items. A row opens as a tab in that project, and each list collapses and remembers it.
- PR tracker 0.7.0 lists each project's open pull requests, yours first and then the ones waiting for your review, and opens one as a tab with its description, checks and review threads.
- Work item 0.4.0 opens a work item as a tab with its description, comments and linked pull requests, opens Updates as a tab with read and unread, and lists your work items in the current sprint under each Azure DevOps project.
- New Tasks plugin: tasks per project in a sidebar and on a board, which Claude can list, add and update.
- Notes is now Project info: a table of facts about the project and Markdown sections with tables, lists and images. Claude can set facts and write or add to sections, but never replaces a section you edited.
- Opening the same item again goes back to its tab instead of opening another one.
- Plugin SDK 0.9.0 adds `contributes.projectSections`, `wings.projects()`, `wings.onProjects()` and `wings.setProjectSection()`. wings-plugin-ui 0.3.0 adds a `Markdown` component.

## 0.1.22 · 2026-10-13

- Terminal tabs reopen in the folder you left them in after Wings restarts, instead of the project root. A folder that no longer exists falls back to the root.
- The + button gets a menu next to it: Terminal, then each plugin page that can open as a tab. The command palette lists the same pages as "New tab: <name>", and Settings, General picks what plain + opens. ⌘T still opens a terminal.
- New Notes plugin: notes per project, in a sidebar or a tab. Claude reads them with `notes__read` and adds to them with `notes__write`, which only appends. Notes are kept in Wings, up to 30,000 characters per project.
- Plugin SDK 0.8.0: `wings.project()` and `wings.onProject()` give a page its project, `wings.placement()` says where it's open, `openView(id, { as: "tab", args })` with `wings.viewArgs()` opens a page for one item, and `wings.tabState()`/`setTabState()` keep state per tab. Views can set `newTab` to show in the + menu and `multiple` to open more than one tab. Panes from `onPanes` carry their `projectId`.
- wings-plugin-ui 0.2.0 adds layouts for full tab pages: `Page`, `Toolbar`, `ListDetail`, `Board` with `BoardColumn`, and a resizable `Split`.

## 0.1.21 · 2026-10-13

- The docked right sidebar fills the window's height again. In 0.1.20 its page shrank to its header.
- Check for Updates is in the command palette too.

## 0.1.20 · 2026-10-13

- Fixes a graphics memory leak: closing the focused pane or tab kept its terminal's graphics memory for good, about 33 to 66 MB each time. The WebGL renderer's cursor blink timer outlived it (a bug in @xterm/addon-webgl 0.19.0, fixed upstream but not yet released), so Wings now blurs a terminal before dropping its renderer.
- Work item 0.3.0 tells you about Azure DevOps updates: a state change on your work items, comments that mention you on work items and pull requests, and activity on pull requests you created or review (new comments, votes, completed or abandoned, you added as a reviewer). It checks every 2 minutes, also while Wings is in the background (Settings, Work item: off, or 5 to 30 minutes), only reads, and lists each update with a link in a new Updates sidebar. Work item 0.3.1 uses its own icon.
- The right sidebar peeks like the left one: with it closed, pointing at the window's right edge floats the plugin page you opened last over the panes (switch between opened pages from its tabs) without resizing them or reloading the page. Nothing happens until you have opened a plugin sidebar once.
- Idle Claude Code and opencode agents go to sleep after 15 minutes out of sight (Settings, Agents: off, or 5 to 60 minutes), which frees about 400 MB each. Wings ends them with their own `/exit`, never by killing them, and leaves alone any that is working, needs you, has text typed in its prompt, or isn't started straight from the pane's shell. A sleeping agent stays in the Agents list like one restored at startup, and opening its pane or row resumes the session in place (`claude --resume`, `opencode --session`). Sleeping agents survive a restart.
- An agent's row no longer disappears for a moment while it is being resumed, at startup or when you click it: it shows "resuming…" until the agent is detected, then becomes the live row in the same place, and says "couldn't resume, retry" after 20 seconds without one.
- Plugins can pick from more title bar icons. Sprint board 0.4.1 uses a kanban board.
- A plugin can ship its own icon: `"icon": "icon.svg"` in `wings-plugin.json`, a file up to 32 KB. It replaces the puzzle piece in the Plugins list, and a sidebar, view or panel that leaves out `icon` uses it on its title bar button and tab. Wings draws it as a one-colour mask, never as part of the page, so nothing in the file can run. The built-in plugins ship one each (Changes, Dev servers, HUD, PR tracker, Session detail, Work item, Worktrees).
- Worktrees 0.1.3 looks again when a first check of a folder failed, for example while the plugin was waiting for approval, instead of showing "No git repo" until restart.
- The notification list scrolls inside its popover instead of running past it.
- A welcome when no project is open: four steps (add a project, run an agent, the command palette, jump to the agent that needs you) with your actual shortcuts and an Add a project button. The palette already lists every shortcut.
- Title bar: plugin buttons are set apart from the bell, Past sessions and Settings by a hairline, and the plugin overflow button has a tooltip. Sidebar project rows show a focus ring when reached with the keyboard.

## 0.1.19 · 2026-10-12

- Diff review comments only go to a pane running an agent. A plain terminal is no longer offered: the quoted code would have run there on Enter.

## 0.1.18 · 2026-10-12

- Command palette on ⌘K (Ctrl+Shift+K elsewhere): search commands, projects, tabs, agents (needing you first), recent sessions and plugin buttons. Tab, Pane and View menus in the menu bar with their shortcuts, ⌘]/⌘[ for next/previous tab, and a global shortcut (default ⌃⌥J, changeable or off in Settings, Keyboard) that brings Wings forward and goes to the agent that needs you. All of them share one list of commands.
- Rename and reorder: double-click a tab or pane title (or right-click, Rename) to name it; Enter saves, Esc cancels, empty goes back to automatic. Tabs that would read the same get the shortest folder tail that tells them apart, else a number. Drag tabs to reorder them, drag projects in the sidebar to reorder them, and drag a pane by its header onto a tab (or +) to move it there, or use the right-click menus (Move left/right, Move up/down, Move to tab). Move Tab Left/Right are in the Tab menu and palette. Names and order are saved with your tabs.
- Review a diff and send it back to the agent: hover a line number in the diff viewer and click the + (drag for several lines) to comment. Your comments list in a side panel; Send to agent pastes them as one message (file:line, the quoted code, your comment) into the agent in that repo, or the pane you pick, without pressing Enter. Send and submit presses Enter too. Plugins can pass `cwd` to `openDiff` so the right agent is preselected: Changes 0.2.4 and Session detail 0.4.3 do.
- Notification bell in the title bar: the last 100 times an agent needed you or finished, and plugin notifications, newest first with a Go to pane button and an unread count. A dismissed toast is no longer gone for good.
- New Worktrees plugin: a sidebar listing each open project's git worktrees with merged, upstream gone, uncommitted, locked and missing-folder flags. Removing one, or all merged and clean ones, asks first and never forces; Prune clears the records of folders that are gone.
- New agent in a worktree: right-click a project, New agent in worktree, pick a branch name, the branch to start from and Claude Code, opencode or a shell. Wings runs `git worktree add --no-track -b` into `<project>-worktrees/<branch>` beside the repo (never inside it), lists it under its project and starts the agent in a new tab there. It refuses an existing folder or branch and shows git's error.
- Open in editor: a project's context menu opens its folder in VS Code, Cursor or Zed (their command-line tools) or another editor app in /Applications (Xcode, Android Studio, Sublime Text, JetBrains and more). Choose the default in Settings, Editor.

## 0.1.17 · 2026-10-12

- Less graphics memory for terminals: about 100 MB less from the scrollbars, and the tab you just left keeps its fast renderer for 30 seconds instead of for good. Measured in a WebKit test with 5 panes at Retina size: from about 300 MB of graphics to about 120 MB.
- Plugins skip sending a badge or sidebar label that hasn't changed.
- Plugins can tell when Wings is in the background with `wings.onFocus`. Changes 0.2.3, Work item 0.2.2, PR tracker 0.6.3, Dev servers 0.2.3 and HUD 0.3.4 stop their checks while you're in another app and check once when you're back. `wings-plugin-sdk` 0.7.0 adds `pollWhileFocused` for this.
- Dev servers checks ports every 15 seconds instead of 5, looks again a few seconds after a pane changes, and its badge has Refresh.

## 0.1.16 · 2026-10-12

- Fixes high CPU use: once a program in a pane was refused a protected folder, Wings kept checking that folder itself, and each check was refused and logged again, thousands of times a second.
- Plugins resending a badge or sidebar label they already showed no longer re-render the app.
- Point at the window's very left edge to show the closed sidebar.
- A mouse wheel scrolls the tab strip sideways.
- opencode panes show opencode's logo and the same working, needs-you and done states as Claude.
- Plugins has Update all next to Check for updates.

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
