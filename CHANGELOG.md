# Changelog

What changed in each Wings release, newest first.

## 0.2.110 · 2026-11-25

- **Tabs shrink before they scroll**: tabs get narrower, down to 100 points, before the strip scrolls, so five tabs fit a 1000 point window instead of cutting the first to a letter.
- **Cleaner Issues board**: a card's delete and move buttons show while the pointer is on the card (Issues 0.31.4), and plugin pages can do the same for their own cards.
- **Checkboxes you can see**: an unchecked box in a plugin page has a clear edge, where it was nearly invisible.
- **Issues 0.31.3**: an issue with no description offers to add one under its title, not in the middle of the page.

## 0.2.109 · 2026-11-25

- **Past sessions without automated runs**: sessions a program started through the Agent SDK, like automated security reviews, no longer fill Past sessions and its search. Your chats and terminal sessions stay.
- **Issues 0.31.2**: a commit or pull request command with a character like "…" or "é" no longer crashes Issues, and a line of em dashes in an issue no longer crashes its page.

## 0.2.108 · 2026-11-25

- **Faster past session search**: a search no longer lists every session folder first, only the ones where a session came or went, and it scores past sessions about 8 times faster. A busy disk no longer stalls a search for seconds.
- **Segmented switches fade**: choosing Files or Graph, Changes, Branch or History, and the like moves the highlight with a short fade instead of a jump.
- **Changes 0.17.3**: the totals sit in a plain line with a small View all, so the files start higher.

## 0.2.107 · 2026-11-25

- **Live tooltips**: the Settings button's tooltip counts on while indexing, instead of keeping the number it showed first.
- **Tab pages**: the project bar's line no longer runs past the rounded corner of a tab page like Claude chat.
- **Room in the message box**: a plugin's output style goes by its own name ("Concise output"), and the agent picker is a robot button until you pick an agent, so every button shows whole. Claude chat 0.55.4.
- **Motion**: switches slide and folding chevrons turn, at once under Reduce motion.
- **A log of errors**: opened from the Finder, Wings keeps its errors in wings.log in its data folder.

## 0.2.106 · 2026-11-25

- **Better past session search**: common words in a search barely count now, so a long session no longer comes first just because it contains every word of your question somewhere. Searching by meaning and by words are blended the way the docs search does it. Exact searches, like an id, a quoted phrase or a rare word, still find their session first.
- **Dropdowns close on a second click**: clicking the button of an open dropdown or menu closes it instead of opening it again, in Settings, the new-tab menu, project sorting, the Add menu, plugin menus, Open in… and Past sessions.

## 0.2.105 · 2026-11-25

- **Indexing shows on the Settings button**: while past sessions are indexed, the sidebar's Settings button fills up from left to right, and pointing anywhere on it shows how many are done. The update button's hover text now shows anywhere on it too.
- **Less indexing while you work**: a session that is still being written is indexed once it has been quiet for five minutes, not again every minute.
- **Huge queued messages stay small**: a long paste waiting for Claude shows its first line with More, opens in a box that scrolls inside the card, and its hover text is cut short. Claude chat 0.55.3.

## 0.2.104 · 2026-11-25

- **Search past sessions by meaning**: Past sessions and the agents' history search now also find a session by what it was about, not only by its words, with a small model that runs on this Mac. Wings indexes your past Claude Code and OpenCode sessions in the background, subagents included, and pauses while you type. A thin line under Settings in the sidebar shows how far it has got, with the count on hover, and Settings, Agents has its status and an off switch. The model (34 MB) downloads once, the first time.

## 0.2.103 · 2026-11-25

- **Long branch names fit**: in the Changes graph and History, a branch or tag name too long for its row ends in "…" and shows whole on hover. A commit with several names shows one and a "+N" listing the rest, so its message always stays readable. Any plugin's badge now shortens the same way and can carry a hover text.
- **Changes 0.17.2**: the above.

## 0.2.102 · 2026-11-25

- **Pinned plugins**: pinning a plugin to the top bar works again, and every pinned one shows. When their names would take more than half the bar or don't fit, they switch to icons, and the ones that still don't fit go in the "…" menu. The right panel's header is as tall as the project bar beside it.
- **Agents don't move you**: a terminal an agent opens no longer takes you to it, also when the agent runs in a Claude chat tab or opens it in a project you haven't opened yet.
- **Claude chat 0.55.0**: drag the edge of the terminal column beside a chat to resize it, double-click it to reset. `/login` and the "Not logged in" line start the sign-in for the chat's account, then the chat picks up again and resends your message. After Reconnect in the MCP dialog, a server shows "Connecting" and then its real status. An idle chat with terminals beside it no longer redraws every frame.
- **Claude accounts 0.6.0**: a signed-out account says "Not signed in" with a Sign in button, and the inbox tells you when an account gets signed out.

## 0.2.101 · 2026-11-25

- **Flush panes**: panes sit edge to edge and share one thin line instead of floating as separate cards, and the plugin panel on the right matches. "Open in Zed" is one outlined button, and plugin buttons show their names until the bar gets narrow.
- **Narrow windows fit**: the project bar, pane headers, an agent's terminal columns, the find bar and the project page give up words and buttons in turn (behind a "…" menu) instead of running past their edge. Menus taller than the window scroll. A pane's title shows its shell's name again.
- **Sidebar text size**: Settings, Text, Sidebar text has Small, Default and Large, and the default is a step smaller than before.
- **One chime per finished agent**: an agent whose background commands keep ending no longer chimes each time; it chimes once until you look at it or open the inbox.
- **Palette**: a recent session says when it was ("5 minutes ago") instead of a timestamp.
- **Claude chat 0.54.0**: terminals Claude opens dock beside the chat. An agent can open a chat on another of your accounts and hand it a session or a task, and message it right away. A chat takes the name Claude gives its session. An opened command says how long it ran, background ones too. Text another agent sends a chat is marked as not yours. What you type as a chat opens isn't lost.
- **For plugin authors**: opening a tab answers its id (`{ "tabId": "tab-N" }`), the one `send_to_agent` takes.

## 0.2.100 · 2026-11-24

- **A selection in Claude chat stays while the chat updates**: text you are selecting with the mouse is no longer lost when a new message, a tool call or the status line redraws the chat.
- **Copying a list or a table reads right**: a list item's bullet or number stays on the line with its text, and a table row's cells stay on one line.
- **Changes 0.17.1**: its icon buttons are the same size as every other plugin's. **Issues 0.31.1**: a long change from an issue's page is sent in parts instead of being refused.
- **For plugin authors**: the SDK has the parts plugins kept rebuilding (`icon_button`, `setting`, `section_label`, `toolbar_titled`, `ui::icons` and more), and `messages::ask`/`answer`, `publish`/`want` and `approvals::Approvals` for a page that asks its main. `cargo test --all-features` runs every frame's tests.

## 0.2.99 · 2026-11-24

- **Plugins: Update all is back on the Installed tab** whenever updates are waiting, as well as on Updates.

## 0.2.98 · 2026-11-24

- **Claude accounts is a Wings plugin now**: it lives with the others and updates with them. A copy you installed from its own repo moves over by itself and keeps its approval and saved data.
- **Claude accounts 0.5.0**: the above.

## 0.2.97 · 2026-11-24

- **An agent can close a terminal it opened in a project that isn't on screen**: it closes there, and you stay on the project, tab and pane you were on. When it was that project's front tab, the tab beside it is in front when you go back.

## 0.2.96 · 2026-11-24

- **A new Plugins page**: Installed, Browse and Updates with a search, an Add plugin menu for a GitHub link or a file, calmer rows with one status each, and a page per plugin with what it adds to Wings, what it can reach and its README. In a narrow window, Settings shows its sections as icons.
- **Sidebar polish**: a project or agent that finished while you were away is bold until you open it, the current project and its worktrees share one card, the Projects header has a filter menu (hide idle projects or worktrees), and agent rows show the model and how long ago they changed.
- **Wings opens where you left it**: its size, place and screen are kept when it quits, and it opens centered when that screen is gone.
- **Claude accounts stay in step**: a change to the default account's settings reaches the accounts that share its setup right away.
- **Claude Code plugin panes read like Wings panels**: one header named after the pane, the content from the top, a plugin's choices as full-width rows that wrap, picked-one-of filters as a segmented control, and no plugin can pin a row to the pane's foot. Long button labels wrap instead of being cut short.
- **Issues: no caps**: a project takes any number of issues and an issue any number of log entries. Board columns, the list and an issue's activity show more with Show more, the issue's … menu opens over the page, and deleting asks in a dialog.
- **Bigger limits**: plugin pages hold 20,000 elements (boards draw only the cards in view), a plugin keeps 500 MB, pages send each other up to 1 MB, a text box holds 200,000 characters, a copy takes 16 MB, the inbox keeps 1,000 notifications, and an agent reads the last 2,000 lines of a pane. A terminal keeps output across restarts in step with your scrollback setting, up to 10 MB.
- **Claude chat**: every picture shows, not only the newest 12. A long permission request can be read whole and allowed. A subagent's tab reads its whole run. A resumed chat says when its earliest messages weren't loaded. A review comment that can't be kept says so. @-mentions know up to 200,000 files.
- **PR tracker**: a review comment's code no longer loads forever, and a CI run's log shows earlier lines with Show earlier.
- **HUD** lists every Claude session, and one that ended says so instead of showing another session's numbers.
- **Schedules**: a run in a Claude chat keeps its prompt's lines, up to 20,000 characters. **Changes** opens diffs up to 8 MB.
- **Safer plugins**: Wings signs the plugin code it keeps compiled and prunes what no plugin uses, counts a plugin's calls against its time budget, bounds what one plugin can hold, and runs a plugin's calls on threads of its own. A plugin that stops says what happened. After an update, the first plugin page asks once for keychain access to "dev.wings.app.wasm-cache"; Deny only turns the cache off.
- **Terminal server v10**: terminals and chats running now stay on the old server until they end.
- **Build a plugin with cargo**: `cargo install cargo-wings-plugin`, then `cargo wings-plugin new`. The SDK is `wings-plugin` on crates.io. The npm packages and the old TypeScript code are gone.
- **Claude chat 0.53.0, Issues 0.31.0, PR tracker 0.35.0, HUD 0.10.0, Schedules 0.11.0, Changes 0.17.0, Share 0.2.1**: the above.

## 0.2.95 · 2026-11-24

- **Claude chat: a picture shown large is centered** in its dialog, which plugin rows can now ask for.
- **Claude chat: a picture in a subagent's sidebar opens over the chat**, at the chat's size, instead of inside the narrow sidebar.
- **Claude chat: a Claude Code plugin's pane can fill its height**: a box that grows (`flexGrow`, Ink's `Spacer`) or is 100% high takes the room left, so a pane's buttons can sit at its foot.
- **Claude chat 0.52.0**: the above.

## 0.2.94 · 2026-11-24

- **A Claude Code plugin's pane opens in its chat's project**: a pane a chat in another project opened, like a teach-back lesson, showed in the sidebar of the project on screen. It now opens in the chat's own project's sidebar.
- **No errors when a plugin opens its pane**: the chat telling Claude Code which pane shows and has the keys was refused, as nothing had been clicked, and showed two errors in the chat.
- **Claude chat: a cleaner plugins and MCP dialog**: each group is a card with lines between its rows, a status is a dot and a word at the row's end, a tool takes one line, and a server's tools past 40 open with Show more instead of a count you couldn't open. Servers that need a sign-in in a terminal say so once.
- **Claude chat 0.51.0**: the above.

## 0.2.93 · 2026-11-24

- **Claude chat: "Continue when it resets" continues**: at the reset Wings refused the message, since nothing had been clicked right before it, and the chat showed it sent and kept spinning. Your click on the button now arms that one message with Wings, which lets it go at the reset. After a restart of Wings the card asks for the click again. A message Claude Code never got says so instead of spinning.
- **Agents in one-off terminals**: an agent started in a terminal from the sidebar's Terminal row shows in the Agents list, named after the terminal's folder, and a click opens that terminal.
- **Fewer Wings tools in Claude's list**: rarely used tools (disk usage, failing checks, changed files, schedules, project info, worktrees, performance) are commands Claude runs with Bash instead of MCP tools, which leaves 21 of 45 in its tool list. `wings tools` lists every command and `wings call <tool>` runs any of them. Plugins mark a tool `cliOnly` in their manifest.
- **"Load Wings tools at session start" applies to every Claude account**: it only ever changed the default account's setup.
- **Share** (new plugin, off until you turn it on): Claude can share an app running on a local port at a public OpenTunnel link, after you allow it in Wings. A terminal whose app is shared shows the link with Open, Copy and Stop, and the link comes down when the app stops. Needs `opentunnel` (`brew install anomalyco/tap/opentunnel`).
- **Claude chat 0.50.0**: the above.

## 0.2.92 · 2026-11-24

- **Claude chats are kept apart from the terminal server for real**: 0.2.90 moved each chat's Claude into a process group of its own, but the terminal server that was already running kept its old code through the update. This release brings terminal server v9: terminals and chats that run now stay on the old server until they end, and new ones start on the new server.

## 0.2.91 · 2026-11-24

- **Claude chat: plugins installed with another show as installed**: after an install, the plugins dialog reads its list again, so plugins that came with the one you installed no longer offer Install.
- **Claude chat: a notification says whether it reached your phone**: its row reads "Push requested", or why it wasn't pushed.
- **Claude chat: a skill served by an MCP server** has an MCP badge on its row.
- **Claude chat: a tool call with no name** shows as "Unnamed tool", and a refused call's error shows without Claude Code's `<tool_use_error>` tags.
- **Claude chat 0.49.0**: the above.

## 0.2.90 · 2026-11-24

- **Claude chats keep running through updates again**: a chat whose terminal server had stopped ran Claude inside Wings, so the next update or restart ended it. It now starts a new terminal server instead. Chats now run in their own process group, so nothing a chat starts can stop the server and every other chat with it. Wings logs the server's starts, failures and endings in `terminal-server/app.log`.
- **Claude chat: questions read better**: each option is a row with its whole description under its label, the header is quiet text, Something else is the last option and opens a field, and there's one main button.
- **Claude chat: commands anywhere in a message**: type `/` after a space to complete a command partway through your message. Tab inserts the top match, the arrows pick one, and Enter still sends what you typed.
- **Claude chat: a command cut off by a restart** shows as interrupted, instead of spinning forever.
- **Claude chat: a pasted picture isn't lost** when the message box moves as the tab narrows.
- **Tools**: the "Check what Claude chat needs" button is gone.
- **Claude chat 0.48.0**: the above.

## 0.2.89 · 2026-11-23

- **Tools update in place**: Update runs in the tool's row with a spinner and keeps Settings open, then shows the new version. If it fails, or needs a password, the row shows the last lines of what it said with Run in terminal and Try again.
- **Tools from another Homebrew tap**, like OpenCode, show their newest version and update by their full name, instead of "couldn't look up the newest".
- **Claude chat: Open a terminal an agent used**: a call that opened, read or messaged a terminal has an Open button that shows it, and says so once that terminal is closed.
- **Claude chat 0.47.0**: the above.

## 0.2.88 · 2026-11-23

- **Claude chats restart in their own folder**: after an update or restart, a chat in a project that wasn't on screen started Claude Code in the project that was. A chat now saves its folder, an older chat takes it from its session, and a plugin tab is told its own project rather than the one on screen.
- **Ultracode in the model menu**: a switch at the end of Claude chat's model menu, and the model button shows "· Ultracode" in an accent colour while it's on.
- **Slash commands typed fast open**: Enter or Tab in a message box no longer picks from a list drawn before your last key, so `/mcp`, `/hooks` or `/usage` typed quickly opens its dialog, as picking them from the list now does too.
- **Scrolling to the top of a long Claude chat loads older messages again** and keeps your place.
- **Claude chat**: with the session panel in Wings' sidebar the header no longer cuts the title and folder to "…", and file names in the Memory dialog are no longer cut before their description.
- **Claude chat 0.46.0**: the above.

## 0.2.87 · 2026-11-23

- **Tools in Settings**: the command-line programs Wings and your turned-on plugins run, each with your version and the newest, and an Update for the ones installed with Homebrew or Claude's installer, which runs in a terminal tab. Wings checks when it starts and every six hours, and the Tools entry counts the updates waiting. Claude Code's row lists what's new since the version you last read, and in the Wings project opens a Claude chat that compares those changes with what Claude chat supports.
- **Scheduled runs open in the background**: a run, in a Claude chat or a terminal, opens in its own project and leaves the project you're looking at on screen. A chat run used to open in the project on screen.
- **Agents set up the worktrees they make**: Claude can copy a project's local files into a worktree it made itself and run the project's setup command there, once you allow it.
- **Messaging an agent works again after arrows and shortcuts**: arrows, Page Up and Down, Backspace and shortcuts like Ctrl-P no longer count as unsent text, which kept Wings from writing to an agent whose prompt was empty.
- **The keyboard reaches more places**: plugin popup menus, right-click menus and Settings selects take Up and Down, Home and End, a typed letter and Enter, and Escape closes an open select before the dialog. The plugin review and the setup approval take the keyboard when they open and keep Tab inside, as the command palette and the history sheet now do. Escape closes the diff viewer without reaching what's under it, and Page Up, Page Down and letters move through its file list.
- **Cut-off text shows whole on hover** in menus, the sidebar, agent names and the diff file list.
- **Claude chat**:
  - `/goal` shows a bar above the message box with the goal, how long it has run, its missed checks and the latest reason, and Clear. The chat notes each missed check and when the goal is met.
  - Scheduled tasks and `/loop` wakeups read as plain rows, and the side panel lists them with Stop.
  - Skills and MCP prompts that load mid-chat show in the slash menu, and a model's refusal and the retry on another model are noted.
  - An Ultracode button beside the model menu, workflow runs as a tree of their phases and agents, and a new chat can start as one of your Claude Code agents.
  - `!command` runs a shell command and hands Claude its output with your next message. `/skills` lists the skills and sets how Claude sees each.
  - A long chat that names many issues no longer stops with "the page's tree has over 5000 nodes" when you scroll up: Mentioned lists the newest that fit and counts the rest, and Subagents the newest 60.
  - A picture you sent stays yours: a picture in another chat tab no longer takes its place.
  - A long background command's finished line is cut to fit, with all of it on hover, and a long pull request name in Mentioned keeps its number.
  - In a narrow tab the title keeps its room beside the Update pill, and the account shows as its icon. Search shows plain arrows when nothing matches, an allowed block reads "· Allowed", bookmarks show no markdown backticks, the plan page's bar no longer squeezes a title, and the usage note and a failed start's banner no longer touch what's beside them.
- **Claude chat 0.45.0, Schedules 0.10.1**: the above.

## 0.2.86 · 2026-11-22

- **Claude chat moves aside only as far as it needs**: with the session panel open, the chat stays centred where it already clears the panel, and in a narrower tab moves left just enough, keeping the prompt rail. 0.2.85 moved it the panel's whole width even when there was room.
- **Claude chat 0.43.4**: the above.

## 0.2.85 · 2026-11-22

- **Claude chat's panel stops covering the chat**: where the tab has room, the open session panel moves the conversation and the message box aside, so long lines, code and errors no longer run under it. A chat with a prompt rail keeps its rail: it moves aside once there's room for both.
- **Every Claude chat setting shows in the sample chat**: searches, web fetches, subagent calls, other tools, summaries, notes from Claude Code and subagent reports now change the session preview in Settings too.
- **Claude chat 0.43.3**: the above.

## 0.2.84 · 2026-11-22

- **One look across the plugins**: Issues, Schedules, PR tracker, Work item, Changes, Disk usage and Worktrees follow the same design as Claude chat and Settings. Headers name the page with its context greyed after it, actions are icon buttons with tooltips, choices between views are segmented tracks, toggles are checkboxes, and lists are rows in one card instead of a card each.
- **Issues**: New issue is a + button that opens a full-width line under the filters; view, sort and toggles sit in a bar under the header.
- **Long checkbox labels wrap** instead of running past their card in a narrow sidebar.
- **Issues 0.30.0, Schedules 0.10.0, PR tracker 0.34.0, Work item 0.13.0, Changes 0.16.0, Disk usage 0.4.0, Worktrees 0.8.0**: the above.

## 0.2.83 · 2026-11-22

- **Very long chats open**: a plugin page gets ten times as long for each step, so a Claude chat reading back a session with many screenshots no longer stops with "the page ran for too long".

## 0.2.82 · 2026-11-21

- **Long chats keep working**: a plugin page may now use up to 512 MB, up from 64 MB. A long Claude chat with many screenshots ran out of room and stopped with an error.

## 0.2.81 · 2026-11-21

- **The hover cue follows the pointer**: the bar under the pointer while a card is on its way moves with the pointer instead of staying where it first came onto the row.
- **Every Claude chat checkbox has its example**: each fold under "What starts open" shows what it opens again, and turning on Remote Control in new chats shows the message box's Remote button.
- **Per-account limits wrap cleanly**: each Pause and Stop label stays beside its menu in a narrow window.
- **Claude chat 0.43.2**: the above.

## 0.2.80 · 2026-11-21

- **Settings rows fit at any width**: a control too wide to sit beside its setting's name goes under it instead of squeezing the name to a letter a line. Issues' settings were unreadable in a narrow window.
- **Claude chat settings breathe**: the sample chat and the options keep a gap at the line between them, and the "where the session panel sits" sample draws properly in both places.
- **Issues' default view is a menu**: Board, Board by epic and List, with what each shows in the text beside it.
- **Claude chat 0.43.1, Issues 0.29.2**: the above.

## 0.2.79 · 2026-11-21

- **Claude chat in the bar**: Claude chat has its own plugin button. Its Subagent, Claude Code pane and Claude session sidebars no longer show there, since they only make sense opened from a chat.
- **Dithered background by default**: new chats start on the ordered dither. A first chat says once that Settings can make it plain.
- **Bigger plugin settings**: a plugin's settings page opens in a larger dialog. Claude chat's shows a live sample chat beside its options, and one column when narrow.
- **Tidier settings pages**: PR tracker and Issues settings use the same rows and cards as Wings' own.
- **Claude chat 0.43.0, PR tracker 0.33.2, Issues 0.29.1**: the above.

## 0.2.78 · 2026-11-21

- **The command palette sits in the middle**: it's centred top to bottom in the window, and its top stays put while you type and the list shrinks.
- **Claude chat 0.42.1**: Settings shows what each fold checkbox opens, with a preview that follows the tick, in one "What starts open" list that now includes subagent reports.

## 0.2.77 · 2026-11-21

- **Side panels scroll to the end**: a short list inside the panel no longer catches the wheel, so the panel scrolls past it without nudging the list first.
- **The hover cue sits below the pointer**: the loading bar shows clearly under the cursor, above it near the window's bottom edge, and lists no longer cut it off. Clicking a row while its card loads cancels the card, and clicking closes an open one.
- **Subagent reports in the chat**: a report a subagent hands back shows in the chat that started it, in full, with an Open button for the subagent. A setting starts them folded. Long reports stay whole when a chat reloads.
- **Every tool call opens on a click**: clicking any tool row shows the whole call, every input in full and then the output. Also for rows a mod draws.
- **Claude chat 0.42.0**: the above.

## 0.2.76 · 2026-11-21

- **Hover cards wait a moment**: a card opens after 0.6 s, with a thin bar under the pointer that fills while it waits. Moving away before it fills opens nothing, so passing over a list no longer pops a card for every row. Once one is open, the next row's opens at once.
- **Claude chat 0.41.1**: each account's pause and stop can be turned off, or on when the shared setting is off.

## 0.2.75 · 2026-11-21

- **Side panel lists show everything**: Claude chat's Mentioned, Subagents, Bookmarks and Plans sections list every row and scroll inside themselves past about eight rows, where they cut to "N earlier". Plugin pages can cap a scroll's height the same way.
- **The message box stops growing**: past 12 lines it scrolls inside, with the wheel and with the caret.
- **Copy buttons go back**: "Copied" turns back into the copy icon after 2 seconds.
- **Subagent tabs show what was sent**: a message the chat's Claude sends a subagent shows in its tab as it's sent, and one you type there shows as on its way until it arrives. The subagent's final report reads in full.
- **Tool rows open on a click**: clicking a tool call's row opens it, with the whole input, and hovering a cut summary shows all of it. In chats and subagent tabs.
- **Pause and stop per account**: with more than one Claude account, each can have its own pause and stop percentages in Claude chat's settings.
- **New backgrounds**: Black and white, Ocean, Meadow and Dusk pixels, Pixel blocks, Ordered dither, Grainy gradient and Grainy lagoon. Moire is gone, and a chat that used it shows the plain ground.
- **Claude chat 0.41.0**: the above.

## 0.2.74 · 2026-11-21

- **Issues know which chat worked on them**: an issue a Claude chat touches records that chat's session again, so each turn's commits and changed files are linked to it.
- **`wings performance` says what redraws the whole window**: it lists each place that did, with how often, next to the frames and renders.

## 0.2.73 · 2026-11-21

- **A working chat stays light**: a chat's status changing, on every tool call while Claude works, redraws the sidebar and tab strip and no longer rebuilds the chat page. Sidebar lists that change redraw only the sidebar.

## 0.2.72 · 2026-11-21

- **Lighter typing in Claude chat**: a key that changes nothing else on the page no longer rebuilds the chat. The plugin's CPU while you type drops from about a fifth of a core to almost nothing.
- **Quotes copy cleanly**: the bar beside a quote is drawn as a border, so selecting a quote copies only its words. In Claude chat, Tasks and PR tracker.
- **Counters fit their line**: "Thinking · 12s" no longer wraps or leaves a gap as the count grows.
- **Quieter in the background**: Wings looks for running agents less often while it's behind other apps and none is working, and reads your home folder only when it changes. Pixel backgrounds cost less per step.
- **Claude chat 0.40.0, Tasks 0.28.2, PR tracker 0.33.1**: the above.

## 0.2.71 · 2026-11-21

- **Typing stays light in real chats**: a page that answers a key late, as a busy chat does, no longer redraws the page for it, and a panel floating near the message box no longer slows typing. In a chat with a task running, a key costs 2 ms instead of 9.
- **Counters don't redraw the page**: a running task's or subagent's "Running · 12s" ticks on its own, where it redrew the whole chat each second.
- **The session panel keeps its edge**: in a narrow tab it ends above the message box, and when it's taller than the room it has, it keeps its border and scrolls inside. It stands off the page with the dialogs' fill and shadow.
- **Claude chat 0.39.0**: the above.

## 0.2.70 · 2026-11-20

- **Typing stays light everywhere**: a key in any text field, the chat's message box, a plugin page's filter or the command palette, now redraws the field and not the part of the window around it. In a long chat a key at typing pace went from about 14 ms to 2.5 ms.
- **Stop near the limit works**: the setting can now stop a chat's turn without a click.
- **Usage**: opening a chat's usage view refreshes its side panel and Wings' sidebar with the same numbers.
- **Claude chat settings** show previews of each choice and are laid out like Wings' own Settings, with lines between rows. Plugin pages can group rows that way too.
- **Seven new backgrounds**: Topography, Flow lines, Grid, Dot matrix, Silk, Cells and Moire.
- **Claude chat 0.38.0**: the above.

## 0.2.69 · 2026-11-20

- **Usage in the sidebar stays current with less asking**: Claude chat passes on the usage Claude Code reports with its replies, in the background too, and Wings skips its own once-a-minute check for an account a chat just reported. The plan limit's note reads the same numbers.
- **An approval shows as what you allowed**: Allow and retry shows a check and the command, not the long message auto mode needs (Claude chat 0.36.1).
- **Claude chat 0.37.0**: the above.

## 0.2.68 · 2026-11-20

- **Past sessions show their first message again** under sessions Wings named, like the ones run from Claude chat.
- **Typing in a long chat does a little less work**: each character's width is measured once instead of on every redraw.
- **Pausing near the plan limit works in chats**: Wings now reads the usage of each chat's account, and Claude hears the note with every tool call, in subagents too. Before, a Wings with only chats open had no usage numbers, so the note never came.
- **Stop the chat's turn near the limit**: a new setting in Claude chat's settings (off by default) stops Claude's turn as the Stop button does once your 5-hour or weekly usage passes the share you pick, and again when the limit is used up. The pause note's share is yours to pick too, from 70% to 100%.
- **Messages you send a subagent stay in its view**, also after it loads again, and ones still on their way show at the end.
- **Claude chat 0.36.0**: no Open in terminal button, and the above.

## 0.2.67 · 2026-11-20

- **Typing in a long chat is light**: a long page lays out only the messages on screen, so a key in a 120-message chat lays out 9 of them, not all. Any plugin page with a long list gets it.
- **A performance readout**: "Performance Readout" in the command palette shows Wings' CPU, frames per second and what redraws. Agents read the same with the `performance` tool or `wings performance`.
- **Claude Code plugins show more**: notifications, rows placed in the conversation, progress bars and fields that update as you type. A plugin's command typed while Claude works says it runs when Claude finishes.
- **Claude chat (0.35.0)**: the side panel folds into a pill with the project, branch and changes; built-in commands like /usage and /plugins come from one list, and a plugin's command of the same name goes to the plugin; diffs in a sidebar are one column; a subagent woken again shows running.
- **Fixed**: a refused plugin call is now written to the plugins' log.

## 0.2.66 · 2026-11-20

- **Plugin panes load**: a Claude Code plugin's pane, like the feedback plugin's inbox from /feedback-inbox, opens in the sidebar with all of it. A large pane was refused on its way there and stayed on its loading mark. Closing the pane in Wings closes it in Claude Code too (Claude chat 0.34.0).

## 0.2.65 · 2026-11-20

- **Spinners cost nothing while Claude works**: they turn in macOS's own animation layer over the window, so a working chat no longer redraws the window 30 times a second for them. Under a menu or dialog they turn as before.
- **Typing in a chat draws less**: a key draws the conversation once, not twice, when the chat only echoes what you typed.
- **Tab picks a suggestion**: with / commands, @ mentions or # issues listed under the message box, Tab takes the highlighted one instead of typing a tab.
- **Up brings queued messages back**: messages typed while Claude works come back into the box to edit, as in Claude Code (Claude chat 0.33.1).
- **Issues**: Claude names an issue's fields right on its first try (Issues 0.28.1).

## 0.2.64 · 2026-11-20

- **Cheaper frames in a busy window**: Wings moves to GPUI 0.3.13, which no longer re-sorts every button and field on screen in each frame. A long chat at work draws a frame in about half the time.

## 0.2.63 · 2026-11-20

- **Fewer redraws while idle**: Claude chat's session panel turns its spinners without redrawing itself, a plugin sending the same badge, status or sidebar rows again no longer redraws the window, and the glow behind an empty chat moves 10 times a second instead of 30.
- **Verbose follows your list**: Thinking is back in Claude chat's What Verbose opens and now applies in Verbose, and notes from Claude Code (hook output, saved memories) can start open or shut too.

## 0.2.62 · 2026-11-20

- **Far less power while idle**: a spinning status dot anywhere no longer rebuilds the sidebar, title bar and main column 30 times a second. Wings sat at about a third of a CPU core with nothing happening.
- **Backgrounds fill the chat**: the GPU background was drawn upside down, which left a strip at the bottom of the chat and showed it faintly behind the title bar.
- **View modes in Settings**: Claude chat's Output setting is now Normal, Thinking or Verbose, the same switch as the chat's header and Ctrl-O. "What Verbose opens" picks which kinds of output Verbose shows open.
- **Claude Code's version in the top bar**: a quiet pill shows the version, and turns into an Update button when a newer one is out.
- **Queued checks say so**: a check waiting to start reads "queued", not "running", and the side panel names each one and how long it has waited.
- **A smaller pull request section**: hidden when the branch has no pull request, with a Create PR button beside the branch button instead.
- **Fixed**: picking a menu option that hangs below the Settings window closed Settings.

## 0.2.61 · 2026-11-20

- **A working chat costs far less power**: its spinners, working dots and "Working…" line move in a layer of their own, so they no longer redraw the whole conversation 30 times a second. While Claude worked, that kept Wings at about a full CPU core.
- **Backgrounds drawn under the window**: Pixel drift and the gradients draw in a GPU layer beneath Wings, so a step no longer redraws the window. Three new pixel backgrounds: Pixel rain, Checker shimmer and Embers.
- **The plan as a document**: every plan Claude proposes stays open as a page after you approve or reject it, from the side panel or the plan card. Comment on any part, and the comments go back to Claude.
- **Search and bookmarks in a chat**: Cmd-Shift-F finds text in the conversation and jumps between matches. Bookmark any message and jump back from the side panel, also after a restart. A Normal, Thinking or Verbose view switch sits in the header (Ctrl-O cycles it).
- **More @ mentions**: folders, line ranges like `@src/cart.ts#L10-20`, and open terminals, whose recent output goes along with your message once you allow Claude chat to read terminal output.
- **Pull request in the side panel**: the chat's branch shows its pull request with review and checks, and buttons to open it, see why checks fail, merge it (after you confirm), or ask Claude to create one.
- **Allow what auto mode blocked**: a blocked action has "Allow and retry", and the side panel lists what was recently blocked.
- **Questions one at a time**: when Claude asks several, the card steps through them, and Enter in "Something else" moves on or sends. Long questions wrap.
- **Claude Code's version and updates**: Claude chat's settings show the version you run, whether a newer one is out, and an Update Claude Code button.
- **Fixed**: the Side by side button on diffs did nothing, a background change was sometimes lost while Claude was answering, and numbered lists in a plan all read "1.".

## 0.2.60 · 2026-11-20

- **Gradient backgrounds**: Claude chat's Background setting adds five softly moving gradients (Halo, Pensive, Mint, Interstella, Nighty night), drawn by real GPU shaders. Like Pixel drift they move only while Wings is in front and stand still under Reduce motion.
- **Comment on the change**: Review changes in Claude chat's side panel opens everything changed, against the default branch or another local branch. A + on any line starts a comment, and your comments go to Claude with your next message.
- **Side panel sections fold**, and stay folded after a restart, in the chat and in Wings' sidebar.
- **Usage at a glance**: the status line counts tokens while Claude works, each subagent shows its tool calls, each reply says how long it took and how many tokens it used (and its cost on an API key), and the panel shows how long the prompt cache stays warm.
- **Wait out a usage limit**: when a limit stops Claude, a card says when it resets and can continue the task then.
- **Rename a chat** from its header, or let Claude Code suggest a name. Each chat's state follows Claude Code's own summary of the turn.
- **Notes Claude Code sends**: an action auto mode blocked, a hook that failed or stopped Claude, memories saved, missing attachments, and a warning when the disk was too full to save the conversation.

## 0.2.59 · 2026-11-20

- **Pixel drift**: a Background setting in Claude chat puts a dim, slowly moving pixel pattern behind the whole chat. It moves only while Wings is in front and stands still under Reduce motion, and a step redraws the pattern alone, not the conversation.
- **Start a chat in a worktree**: a Worktree button beside the branch button makes a worktree of the project for the chat's first message, listed under the project like the ones Wings makes.
- **Run a schedule in Claude chat**: a Claude schedule can open its run as a chat tab that starts with the prompt, instead of a terminal. If the chat can't open, it runs in a terminal and the run says why.
- **# finds an issue**: typing # in Claude chat's message box lists the project's issues and the repo's open GitHub issues, and a pick puts its key or number in.
- **Plugins can call programs on this computer**: a plugin may ask for http addresses on this Mac with a port, which the approval dialog lists apart from web addresses. A plain address only reads; each write is its own entry in the dialog. Tokens never go there, and no redirect is followed.
- **Dialogs keep the keyboard**: Tab and Shift-Tab stay inside an open dialog, and closing it gives the keyboard back to what had it. Escape closes every dialog the same way.
- **Fewer notifications from Claude chat**: only when Claude asks for permission or a question waits for you, not after every turn or finished subagent.
- **Mentioned issues follow their status** in Claude chat's side panel, and a long tool row from a Claude Code plugin is cut short at the conversation's edge.

## 0.2.58 · 2026-11-20

- **A new Claude chat opens in the middle of the tab**, with "What should Claude do?" over a soft glow that drifts slowly. It stands still under Reduce motion and while Wings is in the background.
- **Edit and Write requests show a real diff** before you allow them: the lines around the change with their numbers, colours, and a button that opens it in the diff viewer. A new file reads as all added.
- **Side by side diffs in Claude chat**, for edit requests and finished edits, with a button on the card to switch between one column and two.
- **Switch branches from the message box**: the branch button finds local and remote branches as you type and switches to the one you pick. Git's own message shows when it refuses.
- **Stop with a message queued stops the turn and runs your message next**, as Claude Code does. With nothing queued it just stops.
- **Agents keep their place in the sidebar** when you click one, instead of jumping to the top.
- **Long file names stay on one line** in the Edited files card, and the folder before them is cut short first.
- **Claude chat says when changing effort starts costing a fresh prompt cache**, which Claude Code reports when the server stops taking effort with each message.
- **Escape in a dialog closes only the dialog**: in Settings and the sidebar's remove dialog it also reached the terminal under it.

## 0.2.57 · 2026-11-19

- **Remote Control is on in every new Claude chat**, with a setting in Claude chat's settings to leave it off.
- **The prompt rail glides** from one prompt to the next as you scroll, instead of jumping.
- **No square corners behind rounded cards**: a plugin page in a tab or sidebar card, and a terminal's bottom corners, follow the card's curve.
- **Pictures and the right-click menu work in a subagent's pane**, and a closed subagent tab still names its model.
- **Menus, lists and cards open where there's room**: they flip above their button near the window's foot, stay inside the window, close the same way on Escape or a press outside, and give the keyboard back. Page menus move with the arrow keys and pick with Enter.
- **Plugins get list rows, sections and dialog actions from Wings**, laid out on one edge with the actions in one order, so pages stop drifting apart. Claude chat's subagents, background tasks, mentions, folders and permission dialogs use them.

## 0.2.56 · 2026-11-19

- **Claude chat's account button stays in its header after Wings updates or restarts**, for a chat that goes on with the Claude Code it ran before.
- **A wrapped line in a code block goes on under its indent**, not at the block's left edge, and a copy keeps each line's indent.
- **Inline code reads as a chip**: a rounded background a little shorter than the line, so code on lines one under the other keeps apart.

## 0.2.55 · 2026-11-19

- **Remote Control in Claude chat**: /remote-control or the button by the model menu turns it on, and its menu opens the session on claude.ai, copies the link or turns it off.
- **Select text across paragraphs and messages** in plugin pages by dragging, and select inside code blocks. Cmd-C copies it all.
- **The session panel is back in the chat** by default. A Session panel setting moves it to Wings' right sidebar, which now shows your usage as soon as you switch accounts, before your first message.
- **Claude chat looks more like the rest of Wings**: one width for the usage meters, rows and headings on one edge, buttons instead of plain-text actions in dialogs, aligned status pills and columns in the plugins, MCP, hooks and permissions dialogs, and a cleaner rewind dialog.
- **Clearer words in Claude chat**: "Running" while calls still run, failed background tasks counted as failed, model names instead of ids, and a one-line message when Claude Code stops.
- **Dialogs keep a margin from the window's edges**, and a large picture fits its dialog.
- **Page menus with no room below open upward**, and the right sidebar's header lines up with the project bar.

## 0.2.54 · 2026-11-19

- **Claude chat's account sits in its header**: a profile button names the account the chat runs under, and its menu checks it and lets you pick another.
- **Claude chat's side panel is a Wings sidebar** on the right, beside Changes and the others, so the conversation has the whole tab. It follows the chat in front.
- **Past sessions in Claude chat opens Wings' own Past sessions**, with search, all projects and running sessions. Resume there goes on in the chat.
- **Open in terminal swaps the chat tab for a terminal** instead of leaving an empty chat behind.
- **New chat, resume and fork open in a new tab while the chat is busy**, so a running turn keeps going.
- **The chat's header shows its state** with the same dot as a terminal pane's, and the folder and branch after the title. A long title is cut short instead of running under the buttons.
- **Shortcuts in Claude chat**: ⇧⌘N new chat, ⌘Y past sessions, ⌥⌘B the session sidebar, ⌥⌘T open in terminal. ⌘+, ⌘− and ⌘0 size the chat's text.
- **Edited files open in the diff viewer** from a turn's Edited card, **memory and settings files open in your editor**, and **pull requests open in Wings' pull request tab**.
- **Other agents can find and message a Claude chat** through Wings' session tools, as they can a terminal's.
- **Clicks in a plugin page go ahead of work it has queued**, and are no longer dropped while Claude streams.

## 0.2.53 · 2026-11-19

- **Start a Claude chat under another Claude account.** The message box has an account menu when you have more than one. In a new chat it restarts under the account you pick, and in a chat with messages it opens a new chat beside it.
- **Mod panes open in their own sidebar**, full height, instead of squeezed into Claude chat's side panel, which now lists them with Open. A pane a mod opens, like Feedback, shows there on its own.
- **Mod rows stay inside the conversation.** A long tool call row from a mod is cut short at the edge, and a mod's buttons wrap onto a new line instead of running over each other.
- **Claude chat's side panel scrolls** when it is taller than the window.
- **The Changes sidebar follows the chat on screen**, so it shows the project the chat works in, not the last terminal's. The commit graph, pull requests and sprint pages follow it too.
- **More shows on a queued message only when it is cut off.**
- **View changes works when Claude edits the same file more than once in a turn.**
- **The Usage dialog keeps showing your usage credits** after they turn off at the monthly limit.

## 0.2.52 · 2026-11-19

- **Claude Code mods can restyle Claude chat**: a tool call's row (with its line under the result), your messages and Claude's, and the line that shows what Claude is doing.
- **Right-click in Claude chat opens a menu**: copy or open a link, copy a code block, and the rows your mods add, like "Summarize this page", which can fill the message box for you.
- **Select text in plugin pages** by dragging (double-click takes a word) and copy it with Cmd-C or from Claude chat's right-click menu.
- **A mod's theme colours Claude chat**: its text, surfaces, accents and a still backdrop.
- **Mods can add to Claude's questions and restyle a folded run of tool calls** ("Ran 2 commands").
- **The MCP servers dialog is easier to read.** Each server shows its status as a badge, and its tool count opens to list the tools with what each does. Servers that need a sign-in fold into one line when there are many.
- **The Usage dialog is easier to scan**: each limit stands apart, tips show as cards, skills, plugins, MCP servers and agents sit under their own headings, and the session's totals are grouped.

## 0.2.51 · 2026-11-19

- **Claude chat shows your Claude Code mods**, as Claude Code Desktop does: a plugin's status line and toasts over the message box, its log lines in the conversation, what it adds to the hint line and mode labels under the message box, and the panes it opens in the side panel, with buttons, text fields and dropdowns that work.
- **Tabs come back after you approve a plugin update.** When an update asks for more access, the plugin's tabs close until you approve it, and now reopen once you do.
- **Claude chat's Mentioned list only has the pull requests and issues you and Claude wrote**, not ones a command printed while looking at another project.

## 0.2.50 · 2026-11-19

- **Claude chat lists the pull requests and issues a conversation mentions** in its side panel, newest first. Hover one for a card with its title, status, author, branch, checks and review, and click a pull request to open it in the browser. Issues come from Wings Issues, GitHub pull requests from `gh` and Azure DevOps ones from `az`, so Wings asks you to approve that access once.
- **A large picture in Claude chat fits its viewer** instead of being cut off when a sidebar is open.

## 0.2.49 · 2026-11-19

- **Claude chat's prompt rail stops above the message box** instead of running down beside it.
- **A long prompt rail scrolls** rather than squeeze every tick into the window: ticks stay at least 12 points apart.
- **The rail's mark stays on the message you're reading** while older messages load above it.

## 0.2.48 · 2026-11-19

- **Claude chat shows a long session all the way back.** Scroll up and older messages keep loading, where it used to stop at "Older messages aren't shown here". Scroll back down and newer ones return. **Jump to the latest**, over the message box, or sending a message takes you straight to the end.
- **Plugin pages keep your place** when what's above or below the part you're reading changes.

## 0.2.47 · 2026-11-19

- **Claude chat keeps running through a Wings update or restart.** Its Claude Code runs in Wings' terminal server, like your terminals, and the reopened tab picks it back up. Two tabs never run the same session at once: the second one says it's already running elsewhere.
- **Claude can make a worktree through Wings**, for a new branch or one that already exists, like a pull request's. It gets your project's local files and setup command, as a worktree you make yourself does.
- **The setup command in a terminal Claude opens runs only if you let Claude run commands in new tabs.** Otherwise it waits for the next terminal you open there.
- **/plugins, /mcp, /usage, /context and the other commands Claude chat answers itself show in the slash menu**, and commands that start with what you typed come first.

## 0.2.46 · 2026-11-19

- **Text in narrow pages lines up with their header and message box**, as in the subagent sidebar, instead of running close to the edge.
- **The prompt rail marks the message you're reading**: the one above a line a third down the view, and the last one once you're at the end, where it marked the one before.

## 0.2.45 · 2026-11-19

- **Claude pauses near your plan's limit.** When your 5-hour or weekly usage passes 90%, Claude is told to finish the step it's on and ask you before more work, in every Claude Code session in Wings. Past the limit it's told to stop, since more work is billed as extra usage. Turn it off in Claude chat's settings.
- **Choose what Detailed opens**: runs of tool calls, command output, file reads, edits, search results, web fetches, subagents, thinking and summaries each have a checkbox in Claude chat's settings.
- **Code is highlighted in Claude chat**: code blocks in replies, files Claude reads or writes, and edits.
- **Long code blocks fold** after 20 lines, with "Show N more lines" to open the rest.
- **A file Claude reads shows its content** in the chat, folded like any output.
- The prompt rail leaves a little room before the conversation.

## 0.2.44 · 2026-11-19

- **A new prompt rail in Claude chat**, right against the left of the conversation: a tick for each message you sent, the one in view longer. Hover a tick to see the message and the start of Claude's reply, and click it to jump there.
- **Commands read on one line in Claude chat**, without the long gaps an indented script left in them.

## 0.2.43 · 2026-11-19

- **A Claude chat you open again shows its whole history**, with the messages you sent while Claude was working. Before, a long session showed only its last few messages.
- **The side panel stays open until you press its button.** A click elsewhere in the chat no longer closes it, and the button no longer reopens what that click closed.

## 0.2.42 · 2026-11-19

- **Updating a plugin keeps your sessions.** Its tabs and sidebars reload in place with the new version, and a Claude chat in the middle of a turn keeps going.
- **The Changes sidebar shows every file's diff in one scroll**, with unchanged lines folded until you open them, a picker for uncommitted or branch changes, and a button that opens each file in your editor.
- **The right sidebar holds several pages**, one above the other: Cmd+click a sidebar button to add it below the open one, and drag the line between them.
- **Claude chat:**
  - A rail at the left jumps between the messages you sent.
  - The side panel shows by default, and its header button hides it or shows it at any width.
  - A subagent opens in the right sidebar beside the chat, with a button to pop it out into a tab.
  - Each turn that changed files ends with "Edited N files", with Undo and View changes.
  - File names in Claude's replies open in your editor at the line.
  - The side panel's Changes count matches the Changes sidebar, new files included, and updates as Claude edits.
  - A suggested message taken with Tab comes back when you empty the message box.
- **Folds show what they hold indented under their title**, so a command sits inside "Ran 2 commands" and its output inside the command.

## 0.2.41 · 2026-11-18

- **Subagents open in a tab of their own** in Claude chat, right after the chat's tab: read what the subagent does as it goes, and message it from there. Back to chat stays at the top.
- **Claude chat shows working while Claude works.** The Agents list said idle while Claude thought or ran a command, and in turns a finished background task started.
- **Finished agents and commands show in the conversation** with their time, Open and their summary, and the side panel marks them done. You get a notification when you're in another tab.
- **Claude chat's side panel floats** over the right of the tab instead of pushing the conversation aside. When there's no room it waits behind a header button with a count of what's running.
- **Fork asks first**, and a forked chat says what it came from. Rewind and Fork sit under your message, so they no longer overlap it.
- **A diff from the Changes sidebar opens in the sidebar**, which widens over your panes with a back button. The terminals keep their size.

## 0.2.40 · 2026-11-18

- **Pictures in Claude chat.** Pictures you attach, files Claude reads and screenshots show in the conversation, also in a resumed chat. A click shows one large.
- **Terminals Claude opens from a chat show in its side panel**, live, with Open in a tab and Close, instead of opening out of sight.
- **Links in Claude chat.** An issue key opens that issue in Wings while Issues is on, and a localhost address opens in your browser.
- **More of Claude Code in the chat:** /clear clears the chat, a compaction leaves a divider, retries and API errors show instead of "Working", /rename names the tab, and Claude's thinking folds under its turn. Replies and code blocks have a copy button, and /export, /status, /btw and `/plan <task>` work.
- **Keys and modes:** Esc stops Claude, Up and Down bring back earlier messages, Shift-Tab changes the mode, and fast mode and the output style sit beside the model.
- **Session tools:** fork the chat, or fork from one of your messages; add a folder with /add-dir; move a running command to the background; send a background agent a message; reconnect, sign in to and sign out of MCP servers; and answer an MCP server's questions in a form.
- **Fixed:** under a named Claude account, /permissions and /hooks named `~/.claude/settings.json` as the file they change. They name that account's file now.

## 0.2.39 · 2026-11-18

- **Claude chat works with every Claude Code account.** A session from your work account resumes in a chat instead of failing with "No conversation found": the chat runs Claude Code under the account the session is in, and a new chat under its project's account, as a terminal there does. Its past sessions name the account when it isn't the default.
- **Resume a Claude session in a chat from Past sessions.** The session's menu has "Resume in Claude chat" next to "Resume in this tab", while the Claude chat plugin is on. A plugin view declares it can with `resumes` in its manifest.
- **Claude chat's /hooks, /memory and /permissions can change things.** Add or remove a permission rule, remove a hook, or turn auto memory on or off. Each change shows what it will write, and to which file, before you confirm it.
- **/usage, /cost and /context open as dialogs** over the chat, and the side panel's context and limit rows open them too.

## 0.2.38 · 2026-11-18

- **Rewind in Claude chat.** Each of your messages has a Rewind button with Claude Code's own choices: restore the code, the conversation, or both. It lists the files it writes back before you confirm, and a restored message comes back in the box to edit.
- **Background tasks in Claude chat's side panel**, like a dev server or a test watcher Claude started, each with how long it has run and Stop. A click shows what it printed.
- **Claude chat's message box keeps the keyboard** when Claude Code rebuilds it. Since 0.2.37 it could lose it, and what you typed went nowhere.

## 0.2.37 · 2026-11-18

- **Claude chat looks like part of Wings.** Its tab is named after the session, as a terminal's tab after what runs in it, and its card starts with a slim header like a terminal pane's, with icon buttons in place of the old bar.
- **Claude chat's past sessions open as a sheet** like Past sessions: search them with the words marked, narrow them to today, 7 or 30 days or a branch, and Resume one in the chat.
- **/plugins finds and installs plugins** from your marketplaces in a Discover view, with search and one Install click.
- **A plugin page can't take the keyboard from a terminal you're typing in.** Its dialogs, sheets and fields that focus themselves only take it when it is in the page or its tab already, or right after you clicked or typed in the page.
- **For plugin authors:** a page can show a `Sheet` from the right edge, name its own tab with `wings.setTabTitle`, have a slim `Bar`, mark searched words in a `Text` and show the Claude mark as an `Icon`, and `wings.pastSessions` says how many messages matched. See `plugins/README.md`.

## 0.2.36 · 2026-11-18

- **Claude chat lists your Claude sessions in the sidebar.** A Chats section under each project shows its newest five, and a click continues one in a chat tab. A session a terminal runs is marked running.
- **A side panel next to the chat** shows the folder and branch, what changed (a click opens the Changes sidebar), how full the context is, your 5-hour and weekly limits, and the subagents with their transcripts.
- **The limits are right after a long turn.** They followed only your messages, so a turn that used up the 5-hour limit still showed 67%. A hit limit also shows once instead of twice.
- **A new message box:** attach images with the paperclip, one picker for model and effort, Claude Code's suggested next message in grey that Tab fills in, "Worked for" after each turn, and a "Waiting on 2 subagents" line with Stop.
- **/plugins and /mcp open a dialog** to turn plugins on or off for the chat and MCP servers on or off for the project. **/usage, /cost and /context** open a detailed view, and **/exit** closes the chat.
- **Claude chat settings:** the permission mode new chats start in, Compact or Detailed tool calls, and Small, Default or Large text.
- **Older messages load as you scroll up**, and long tooltips and queued messages show in full.
- **For plugin authors:** a page can show a `Dialog`, scale its text with `textScale`, take Tab in a text box, open a file picker for images with `wings.pickFiles`, and close its own tab with `wings.closeTab`. See `plugins/README.md`.

## 0.2.35 · 2026-11-18

- **Paste images into Claude chat.** Cmd+V puts a screenshot in the message box as a chip, sent with your message.
- **Slash commands and @-files in a list over the message box**, which the arrow keys move through and Enter picks from.
- **Messages you send while Claude works reach it at its next step**, as in a terminal, instead of after the whole turn. Send now has Claude read one at once.
- **Long paths in Claude's replies wrap** instead of running past the edge.
- **Claude chat keeps the model you picked**, hides a skill's text that showed as your message, keeps tool runs folded until you open them, and shows a failed command's error under it.

## 0.2.34 · 2026-11-18

- **Claude chat shows in the sidebar's Agents.** Like a terminal's Claude: working with what it runs, needs you while it asks something, or done. A click goes to its tab.
- **Claude chat starts on your default model and effort**, per model as your Claude Code settings give them, and shows your 5-hour and weekly usage by the context, as a terminal's status line does.
- **For plugin authors:** a tab page can list the agent it runs in the sidebar with `wings.setAgent`. See `plugins/README.md`.

## 0.2.33 · 2026-11-18

- **Agents can add schedules again.** The "Allow this?" question Schedules asks before an agent adds or changes a schedule never showed, so every request timed out. Plugin dialogs now open over the window. Schedules 0.8.1 also tells the agent straight away if Wings can't ask you, instead of making it wait.

## 0.2.32 · 2026-11-18

- **Claude chat's hooks and MCP servers work.** Claude Code in a chat couldn't find programs like node or npx, so its hooks failed and MCP servers didn't connect. Programs a plugin starts now get your shell's PATH.
- **One Stop or Send button in Claude chat's message box.** It's Stop while Claude works and nothing is typed, else Send.

## 0.2.31 · 2026-11-18

- **Claude chat shows what Claude is doing, and for how long.** A line under the conversation says "Running pnpm test · 16s", "Reading checkout.ts" or "Calling wings", and counts up, as Claude Code's own status line does.
- **Smarter permission prompts in Claude chat.** Always allow adds the rule Claude Code suggests, so it stops asking; Deny takes a reason that Claude reads. The card shows the whole command, or the change an edit makes, with hidden characters written out; one too long to show can't be allowed there.
- **Claude's questions and plans get their own cards.** Pick an answer from the options or write your own; approve a plan, approve it and let Claude make edits without asking, or send it back with what to change.
- **Type while Claude works.** What you send waits in a queue above the message box, where Send now gets it to Claude at its next step and × takes it back. Stop clears the queue too.
- **Claude chat tells you when it needs you.** A notification when Claude asks something or finishes a turn, while you're looking at something else.
- **Tool calls drawn by kind.** Edits show their lines added and removed, a written file its lines, a failed command its exit code, and Claude's checklist stays above the message box with what's done.
- **@ mentions files.** Typing @ in the message box lists the project's files that match.
- **Schedules can run once on a date.** Pick "On a date" in the form, or give an agent a date: it runs once at that time, and if Wings was closed then, when Wings next starts.
- **For plugin authors:** a `Text` with `since` ends with how long ago that was and counts it up, without the page sending anything each second.

## 0.2.30 · 2026-11-18

- **Claude chat opens long sessions.** One with a long history stayed on Loading, as it was more than a page may draw: the chat now shows the newest messages that fit, and builds a tool's output only when you open it.
- **Claude chat looks and works more like a chat app.** Your messages are bubbles on the right; Claude's tool calls fold into one line per run, like "Ran 2 commands · Read 3 files"; what Claude Code adds in your name, like a background task's notice, is a quiet line of its own; "Thinking" shimmers while it thinks; new messages rise in. The message box is one rounded box with the model, effort and permission mode in it, how full the context is, and a round send button that turns into Stop. Typing / lists the slash commands.
- **Past sessions in Claude chat.** Sessions lists this project's Claude Code sessions, with a search, and opens one in the tab. New chat starts afresh.
- **Back to a terminal.** Open in terminal goes on with the chat's session in a terminal, once Claude Code in the chat has ended. A Claude terminal's Chat badge won't move a session while Claude is working in it.

## 0.2.29 · 2026-11-18

- **Claude chat (experimental): Claude Code in a chat tab instead of a terminal.** Turn on the Claude chat plugin, then open a chat from the + menu, or click the Chat badge on a Claude terminal to carry its session on in a chat. Replies stream in, tool calls show as cards, permission prompts have Allow and Deny buttons, Enter sends and Shift+Enter starts a new line. A session started with `claude-rtk` goes on with `claude-rtk`.
- **Red messages in plugin pages have a red border.** They had a green one.
- **Long text in plugin pages isn't cut short.** A paragraph stopped at 2,000 characters.
- **For plugin authors:** a plugin can keep a program running and talk to it (`wings.spawn`), close a terminal (`wings.closePane`), keep a scroll area at its end, and send a text box on Enter. See `plugins/README.md`. What a plugin sends a program needs your click or key in its page just before, so a plugin can't answer Claude's permission prompts for you, and it closes a terminal only when you click that terminal's badge.

## 0.2.28 · 2026-11-17

- **Clicking Claude's notification takes you to its pane.** Wings opens on the project, tab and pane of the Claude that sent it.
- **An agent opening a terminal no longer moves you.** The project, tab and pane you're on stay on screen and keep the keys. The terminal opens beside the agent, or as a new tab in its project.

## 0.2.27 · 2026-11-17

- **Notifications come from Wings, and clicking one opens Wings.** They used to go through a script, so macOS showed them as Script Editor's and a click opened Script Editor. The first one asks once whether Wings may send notifications.

## 0.2.26 · 2026-11-16

- **Disk usage's ⋯ menu opens as a menu.** Its Add to Collector, Reveal in Finder and Quick Look showed as loose buttons stacked in the row and ran past the window's edge. They now open in a box under the ⋯, which a pick, a click outside or Escape closes. Plugin pages can use the same menu. Disk usage 0.3.1 needs this Wings.

## 0.2.25 · 2026-11-16

- **A sleeping agent comes back the way you started it.** An agent started through a wrapper script like `claude-rtk` resumed as plain `claude`, without what the wrapper sets. Wings now resumes it through the wrapper. It finds scripts with `claude` in their name that only set variables and end in `exec claude "$@"`. Agents already asleep before this update wake as plain `claude` once.

## 0.2.24 · 2026-11-16

- **Claude Code's `/usage` stops saying "Usage endpoint is rate limited".** Wings asked Anthropic for your usage limits every 5 seconds, which ran into Anthropic's rate limit for both. It now asks once a minute, and after a rate limit it waits as long as Anthropic says.

## 0.2.23 · 2026-11-15

- **A floating terminal resizes from any edge or corner.** Only a hidden square in its top-left corner did before.
- **A menu picks which of an agent's floating terminals shows.** With several in the panel, its header names the one on show. Click it for the others.

## 0.2.22 · 2026-11-13

- **Wings runs no JavaScript.** Plugins run as WebAssembly only, and the JavaScript engine is gone from the app. A plugin that only has a JavaScript build says "Built for JavaScript" in Settings, Plugins and in the sidebar, and needs an update. Sprint board 0.11.0 and Claude accounts 0.4.0 are written in Rust.
- **The HUD's limits are live.** Wings asks Anthropic for each signed-in account's usage every 5 seconds while it's in front, as Claude Code's /usage does, instead of waiting for a session to report. An account whose sign-in has expired keeps the numbers its sessions report.
- **The first launch offers the official plugins.** A new install asks once whether to install them, and each one still asks for your approval.
- **Pull requests can be turned off for one project.** A project's menu has a check for each section a plugin adds under it. With Pull requests off, PR tracker doesn't look the project up at all. Needs PR tracker 0.30.0.
- **The commit graph can show in the Changes sidebar**, beside the files, with a button that opens it in its own window. A narrow sidebar drops the author and then the date. Needs Changes 0.10.0.
- **A plugin's page opens in its own window when it's built to WebAssembly.** The Changes graph window didn't open.
- **A diff that fills the window starts its title right of the window buttons**, not under them.
- **Plugin buttons and text can have a hover text.** Claude accounts gets its tooltips back.
- **A plugin page's toolbar no longer cuts off its buttons.** Too narrow for them, they move to a second line.
- **Without the Issues plugin, the Issues tab says so** and opens Settings on Plugins. The old built-in Issues screen and its `--issues-storage-root` and `--issues-scope` options are gone.

## 0.2.21 · 2026-11-13

- **Every plugin that comes with Wings is written in Rust.** Changes, Work item, Disk usage, Schedules, Issues and PR tracker work as before and run no JavaScript. Needs Changes 0.9.0, Work item 0.12.0, Disk usage 0.3.0, Schedules 0.7.0, Issues 0.27.0 and PR tracker 0.29.0.
- **A plugin page that stops comes back by itself.** A popover, tab or window page that stopped stayed dead, and a sidebar's needed its Reload pressed. Any page now starts again after two seconds, and Reload does it at once.
- **Wings keeps a record of why a plugin stopped**, in `plugins.log` beside its other files.
- **The HUD's limits stay current.** With several Claude sessions open, one that had been idle since your limits were reset kept putting its old numbers back. The HUD now follows the session that answered last.
- **An agent's row in the sidebar opens it** while the one-off terminal is shown. The click did nothing.
- **Schedules asks before an agent starts a ticket.** An agent's start tool made a worktree and opened an agent without asking. It now waits for your Allow in a dialog. An assigned entry has an option to let agents start its tickets without asking.
- **Disk usage moves to the Trash only what you were shown.** Something added to the Collector after you chose Move to Trash stays for the next look.

## 0.2.20 · 2026-11-12

- **Sessions on one Claude login show the same limits.** With the same login in two account folders, like the default one and a named account, the HUD could show an old reading for one of them. Wings now gives every folder signed in to that login the newest limits any of them reported.
- **Session detail is no longer offered.** What it showed is in the HUD, the terminal and Changes.

## 0.2.19 · 2026-11-12

- **Settings, Plugins is easier to read.** Each plugin's row says what it does, and an amber label marks only what needs you: approval, an update, or no native build. No plugin's page is open to begin with. Click a row to open its page and click it again to close it. When a plugin can't be turned on or updated, the reason shows in its row.
- **Claude Code is connected in every account you're signed in to.** Wings connected itself to the default account and to accounts a project uses. An account you only start Claude as from a pane was left out, so its sessions could report no usage limits. Wings now connects those too, on start and from Connect.
- **The HUD says when a session's account isn't connected.** Such a session showed another account's limits as its own. Its strip now shows no limits and says why, and the sidebar names the account of each limit. Needs HUD 0.9.1.
- **Worktrees is written in Rust.** It works as before. Needs Worktrees 0.7.0.

## 0.2.18 · 2026-11-12

- **Terminals look right after a restart.** After Wings restarted or updated, a terminal could show lines its program had already erased, until the program drew again, and some needed closing and reopening. It happened to a terminal whose pane had changed size while it ran, as when a sidebar opened. Wings now redraws each part of a terminal's output at the size the pane had when it was printed. Terminals already open keep the old behaviour until you reopen them, and the top of a very long session can still be off.
- **Plugin updates no longer hit GitHub's limit.** Updating a plugin could say "GitHub's rate limit was hit" although you're signed in with `gh`. Right after a restart Wings could miss your sign-in and stayed signed out until it quit. It now looks for it again.
- **The HUD follows the pane you select.** The HUD sidebar shows the session in the selected pane, and the limits of the account that session runs as come first. Needs HUD 0.8.3.
- **Dev servers is written in Rust.** It works as before. Needs Dev servers 0.5.0.

## 0.2.17 · 2026-11-12

- **Plugins can be written in Rust.** A plugin's background script and its pages can each be a WebAssembly module built from Rust. Wings runs it sandboxed, draws its pages with the same controls, and gives it the same permissions as a TypeScript plugin. It installs from GitHub the same way. This is a first version: a Rust plugin runs with the default time and memory limits, and each of its parts on a thread of its own.
- **Cyclops is the first.** Cyclops 0.9.7 runs its title bar button, its tools for Claude, its sidebar and its popover from Rust. Needs Cyclops 0.9.7.
- **Wings is about 10 MB bigger**, for the WebAssembly engine.

## 0.2.16 · 2026-11-11

- **Plugins no longer stop when the Mac is busy.** A plugin was stopped for good when one of its steps took over a second, even when it spent that second waiting for a busy Mac. Cyclops' sidebar showed "the page ran for too long and was stopped", and Changes said "plugin changes is not running". Only a step that really runs for a second is stopped now.
- **A stopped plugin comes back.** A plugin's background script that stops starts again by itself after 2 seconds, so its badges, tools and actions return without restarting Wings. A sidebar that stopped shows Reload by its error.

## 0.2.15 · 2026-11-10

- **A list row no longer shows twice.** In the native app, a row that changed place in a plugin's list stayed in its old place too, so a Cyclops entry could show twice while the day's total was right. A moved row now shows once. Cyclops needs 0.9.6.

## 0.2.14 · 2026-11-09

- **Disk usage reuses what it already scanned.** Going into a folder, or opening its smaller items, used to scan that folder again from scratch: 20 seconds on several cores for ~/Library. It now answers from the scan you already ran, in under a second, and leaves out what you moved to the Trash since. Needs Disk usage 0.2.2.
- **Scans use about half the CPU.** A scan runs on 4 threads instead of 8, which takes a home folder from about 300% CPU to 170%, for a few seconds more.
- **Wings tools don't wait on each other.** A slow Wings tool call, like a disk scan, held up every other Wings tool in the same Claude session, and could make the session lose its Wings tools. Each call now runs on its own, and each one finds its pane in 0.1 ms instead of 7 ms.

## 0.2.13 · 2026-11-09

- **Plugins use less memory.** A plugin's pages now share one engine, and the plugins that ship with Wings load React once for all their pages, so the installed plugins take about 35 MB instead of 53 MB.
- **Cyclops looks and works like before the switch.** Its project picker can be searched, its Start and End times edit hours and minutes as separate parts, and its entries show their actions in the row. Needs Cyclops 0.9.5.
- **Disk usage no longer spins on /System.** Listing the smaller items of /System, or of the whole disk, used several cores and never finished. It now skips the data volume that the disk already counts, and finishes in a few seconds.

## 0.2.12 · 2026-11-09

- **Agent summaries and session names are back.** Settings, Agents has "Session names and summaries" again: Apple's on-device model, an Ollama model, or off. Each agent in the sidebar shows a line on what its last turn did, and sessions with no title get a short name in Past sessions and the command palette. The choice you made before the switch carries over.
- **Idle agents go to sleep again.** "Put idle agents to sleep" and "Sleep after" are back in Settings, Agents, on after 15 minutes. An idle Claude Code or opencode you aren't looking at ends to free its memory, shows as sleeping in the Agents list, and resumes when you open its tab, also after a restart. One that is working, needs you, has text typed in its prompt or still runs something it started is left alone.
- **Tabs show when a terminal runs something.** A terminal tab with a program running in it, like a dev server, has a green dot after its title, and its tooltip says what runs.

## 0.2.11 · 2026-11-09

- **Older terminal servers clean up after themselves.** After an update brings a new terminal server, the old one keeps the terminals it runs. Once every terminal on it is idle, its shells move to the new server in the same tab with their output kept, idle agents exit and resume their session there, and the old server ends. Settings, General lists the running servers and their terminals, with a Stop button for an older one, and a terminal still on an older server says so in its header.
- **The update dialog says what happens to open terminals.** When an update brings a new terminal server, it says open terminals move over once idle. When the server was started without launchd, it warns that open terminals may stop when Wings restarts.
- **Copy resume command opens the right Claude account.** A session from another account copies with `CLAUDE_CONFIG_DIR` set to that account's folder.
- **Check logs fit the page.** Long lines in a check's log wrap inside the card instead of running past it, and the text lines up after the line numbers. In PR tracker 0.28.4, picking another step shows it loading at once.

## 0.2.10 · 2026-11-09

- **Let terminals read Chrome's data without Full Disk Access.** Wings' terminals now run under a small "Wings Terminals" app, so macOS can keep a grant just for Google Chrome's data, which the Chrome DevTools MCP needs. When a terminal is refused, Wings asks once and opens Privacy & Security, Files & Folders: turn on Google Chrome under Wings Terminals. Terminals started before this update keep running on the old server until they end.
- **Plugin popovers stay open while you type.** Typing in a popover's field, or using a dropdown in it, no longer closes it. A click outside closes it.

## 0.2.9 · 2026-11-09

- **Plugin sidebars follow your project.** A plugin's sidebar or popover switches to the project you switch to, instead of showing the one it opened in.

## 0.2.8 · 2026-11-09

- **Time fields take digits.** In a plugin's time field, like Cyclops' Start and End, type `930` or `0930` and it becomes `09:30`, so you don't type the colon.
- **Tab moves between a plugin page's fields.** Tab and Shift-Tab go to the next and previous field or button, and stay inside a popover.

## 0.2.7 · 2026-11-09

- **Agents come back after a restart.** The sidebar lists the agents that were running before, also in projects you haven't opened yet, and opening one resumes it. Its row says "resuming…" until the agent is back, and "couldn't resume" if it doesn't show.
- **A Claude session you never wrote in starts fresh.** Restoring a pane whose Claude Code session has no conversation yet starts a new `claude` with the same flags, instead of a resume that can't find it.
- **Panes on a drive that mounts late wait for it.** A pane whose folder is on a drive that isn't mounted yet, like an encrypted one, says so and starts in its folder once the drive is there.
- **Terminals can get Full Disk Access.** macOS never asks before it refuses programs in Wings' terminals another app's data, like Chrome's profile that the Chrome DevTools MCP reads. Wings now checks at launch and asks once, and its button opens the Full Disk Access list and shows the terminal server to add. Settings, General has the same button. The terminal server now keeps one path across updates, so the grant lasts. Add it once more after this update.

## 0.2.6 · 2026-11-09

- **Clicking an agent shows its project in the sidebar.** The project list scrolls to the agent's project when you pick an agent in the sidebar, the inbox or the command palette.

## 0.2.5 · 2026-11-09

- **Updates get through when GitHub's download links fail.** Since yesterday those links often answer HTTP 504 for minutes at a time. Wings now fetches the update through GitHub's API instead, so Check for updates and Install keep working.

## 0.2.4 · 2026-11-08

- **See which Claude account a session uses.** Past sessions and the sidebar's Agents list show the account name next to a session from a named account.
- **Resume a session from another account.** Resuming or forking a session in a terminal on a different account copies it over first, so Claude Code finds it. Past sessions still lists it once.

## 0.2.3 · 2026-11-08

- **Tabs reorder by dragging** along the title bar, and keep their order after a restart.
- **Agents come back after a restart.** A tab whose agent is gone resumes its Claude Code or OpenCode session.
- **Closing the window keeps your terminals running**, the same as quitting.
- **Updates look as they did on the desktop.** An update shows in the sidebar and as a toast, and the update dialog shows the download's progress. Checking for app and plugin updates is faster.
- **Diffs are coloured** like the desktop's.
- **Issue pages** cut long linked issue titles inside the properties column, and you edit the title by clicking it.
- **An abandoned Azure DevOps pull request** no longer shows as open or offers Vote.

## 0.2.2 · 2026-11-08

- **Updates install.** The installer took the new version's "ready" signal for a failure and put the old version back.
- **Review comments in pull request diffs.** A diff with review comments opens, with each thread under its line and a reply box.
- **Disk usage's list scrolls** when it holds more files than fit.
- **A one-column diff scrolls down**, not sideways, with the mouse wheel.

## 0.2.1 · 2026-11-08

- **Looks and works as the desktop app did.** The Issues and pull request pages, plugin settings, the project bar's plugin buttons and their menu, the translucent sidebar, and dialogs are back to the desktop's design.
- **Projects reorder by dragging** in the Manual sort, or with Move up and Move down, and keep the order you had.
- **Terminals an agent opens** can go below, beside or floating over it, and hide when you don't need them.
- **A diff docks beside its tab** instead of covering the window.
- **Worktree setup** is back in a project's menu.
- **Agents in every project show after Wings restarts**, not only in the one on screen.

## 0.2.0 · 2026-11-07

- **Wings is now a native Mac app.** It no longer runs in a web view, and its terminals use the Ghostty engine. Your projects, tabs, settings, plugins and saved terminal output come over on the first launch.
- **Terminals keep running when Wings quits or updates.** They run in a background process and are there when Wings opens again. Settings, General turns this off.
- **Images in terminals.** Programs can show SIXEL, iTerm2 and Kitty images.

## 0.1.124 · 2026-11-06

- **Usage limits per Claude Code account.** With more than one account, the HUD keeps each account's limits apart, so it no longer alerts again every time a session of the other account replies. Each terminal's strip shows its own account's limits, and the sidebar lists every account.

## 0.1.123 · 2026-11-06

- **Claude accounts show whether they're signed in.** Plugins get each account's real sign-in from Claude Code, so a logged-out account no longer looks signed in.

## 0.1.122 · 2026-11-06

- **Each Claude Code account keeps its own model.** An account that shares your default setup gets a copy of your settings with its own model kept, so a work seat no longer starts on your personal default model.

## 0.1.121 · 2026-11-06

- **Every Claude Code account gets your setup.** A second account links your default account's settings, plugins, skills and rules, and gets its MCP servers. Turn it off per account in the Claude accounts plugin and the account's own files come back.
- **Start Claude as any account.** Plugins can open a terminal as a chosen account without changing the project's account.

## 0.1.120 · 2026-11-06

- **A Claude Code account per project.** Plugins can set which Claude Code account (a `~/.claude-<name>` folder) a project's new terminals use, so two accounts work side by side. Wings connects to every account in use, and history and session tracking cover all of them. The Claude accounts plugin uses it.

## 0.1.119 · 2026-11-05

- **Terminals in the sidebar.** Each click on Terminal opens a new shell that fills the window and gets its own row above Projects. While it's on screen no tab is selected, and closing (⌘W) closes the terminal, not the tab underneath. Detach one to float it over your tabs: its row stays, and clicking the row brings it back full.

## 0.1.118 · 2026-11-05

- **The quick terminal can fill the window like a tab.** The sidebar's Terminal row opens it in place of the current tab. A button in its header switches between that and the floating panel, keeping the same shell. Going to another tab or project puts it away.

## 0.1.117 · 2026-11-04

- **Scheduled labels stay with their session.** Opening another session in a reused tab clears the previous plugin label, title and resume flags. Sleep, resume and workspace restoration keep the original session's label.
- **Codex shows its live state.** Its Agents row now shows when it is working, waiting for approval or input, or finished, instead of always saying idle. Fast turns and reconnected panes keep the correct state.
- **Correct Codex and Grok Build icons.** Both use their agent marks in panes and Settings, Agents.

## 0.1.116 · 2026-11-04

- **Tabs use the whole title bar.** Tabs share the strip's width instead of stopping at a fixed size, so titles aren't cut short while space sits empty. With many tabs the strip scrolls sideways.

## 0.1.115 · 2026-11-03

- **Agent icons stay visible.** The Agents list keeps each agent's icon while it's working, needs you, finished or idle. Its state indicator sits next to the status text.
- **OpenCode calls use the right project.** MCP and `wings` commands use the calling session's pane, even when sessions in several projects share one OpenCode service.

## 0.1.114 · 2026-11-02

- **Codex, Cursor CLI and Grok Build panes are recognised.** A pane running one of them shows its name and icon, like Claude Code and opencode, and Settings, Agents uses the same icons. Wings doesn't track whether they're working or waiting yet.
- **Worktrees elsewhere on your Mac work again.** Repos found by "Scan this Mac" show which worktrees are merged or have lost their upstream, missing ones can be pruned, and you can remove a worktree there again after the warning. Since 0.1.108 Wings only allowed listing them. Wings still runs only those exact git commands in repos outside your projects.

## 0.1.113 · 2026-11-02

- **Agents can add timed schedules.** Ask an agent to run something every day at 09:00 and it adds the schedule itself, with `schedules__add`, and can change one with `schedules__update`. Wings asks you to allow it first in an "Allow this schedule?" dialog, showing the project, timing, agent and prompt. Nothing is saved if you deny it or don't answer within 100 seconds. Renaming one doesn't ask.
- **Open diffs keep up with your changes.** The diff viewer from Changes, Session, an issue's changed files and the PR tracker updates on its own while it's open, every 15 s while Wings is in front, and right away after a commit or checkout for Changes. A Refresh button in its header reloads it now. The selected file stays selected when the list changes.

## 0.1.112 · 2026-11-01

- **The background terminal test is off.** Wings removes the background item that `WINGS_PTYD=1` added under Login Items, Allow in the Background. After an update macOS stopped that host from starting, so terminals stay inside Wings for now.

## 0.1.111 · 2026-11-01

- **No blank window when the terminal host stops.** With `WINGS_PTYD=1`, a terminal host that crashed or was stopped left the Wings window blank. Now its terminals close and Wings keeps working.

## 0.1.110 · 2026-11-01

- **Terminals that keep running when Wings quits, for testing.** Opened with `WINGS_PTYD=1`, Wings runs its terminals in a background process that macOS keeps running, listed under Allow in the Background in Login Items. Quit Wings and they keep going, and they come back when you open it again. Without the variable nothing changes.

## 0.1.109 · 2026-11-01

- **Code in plugin pages keeps its colors.** The first code block after a language loads, like a `.tsx` snippet in a PR comment thread, came out mostly plain, because tokenizing it took longer than shiki's half-second limit while the grammar warmed up.

## 0.1.108 · 2026-10-30

- **A local log.** Wings keeps what it prints, how long each plugin command took, and any crash in `~/Library/Logs/Wings/wings.log`, up to about 4 MB. Nothing leaves your Mac.
- **Fewer background processes.** The PR tracker lists the pull requests of every GitHub project in one call a minute instead of two per project, and reads a pane's PR from that list. Work items are read in one call for all of them.
- **Work items in the sidebar load again.** The Work items section stayed empty with Azure CLI 2.91, whose `az devops invoke` fails on that call.
- **Highlighted code in plugin pages.** Code in a PR comment thread and in plugin Markdown is coloured by language, loaded only when a page shows code. Code that scrolls sideways gets a thin scrollbar.
- **Plugins can load images.** A plugin can fetch an image or other file that isn't text, so the Sprint board shows the pictures in a work item.
- **Plugins keep to what they asked for.** A plugin's commands run only in your projects or a repo open in a pane, and Worktrees still lists the worktrees of repos elsewhere on your Mac. git can't read a file outside that folder, a saved token is sent only to the address the plugin names for it, `gh`, `glab` and `az` only reach their own service, GraphQL calls can only read, and no plugin can change a CLI's own settings or sign it in as someone else. Plugins whose access changed ask you to approve it again.

## 0.1.107 · 2026-10-30

- **Name an assigned entry.** A Schedules "When something is assigned to me" entry takes a name, shown in the list and the sidebar instead of what it does.
- **Terminal in the sidebar.** A Terminal row under Search opens and hides the quick terminal, and shows its shortcut.

## 0.1.106 · 2026-10-30

- **Quick terminal.** ⌃` opens a terminal in your home folder over whatever you're looking at, without picking a project. Press it again to hide it, and the shell keeps running. Move, resize or minimise it, and close it to end the shell. Change the shortcut in Settings, Keyboard, or find Quick Terminal in the command palette.

## 0.1.105 · 2026-10-30

- **Worktrees under their project.** In the sidebar a project's worktrees come right after it, and its Issues, pull requests and schedules after them.

## 0.1.104 · 2026-10-30

- **Highlighted code.** Code in Markdown, the setup command to approve and the install and PATH commands is highlighted, with a copy button.
- **The code a PR comment is about.** In the PR tab, review threads on GitHub, GitLab and Bitbucket show the lines they were written on, like Azure DevOps threads already did. GitHub threads keep their code after it changes.
- **Copy code from plugin pages.** Code in a plugin page's Markdown, like an issue's description or a PR comment, has a copy button.

## 0.1.103 · 2026-10-30

- **Deleting a worktree is quick.** Delete worktree in the sidebar finishes at once instead of hanging for 15 seconds or more on a big `node_modules`. Its files are deleted afterwards.
- **Every open pull request in the sidebar.** Settings, PR tracker, "Sidebar lists" can show everyone's open pull requests under a project, not just yours and the ones waiting for your review.
- **Issue details scroll.** On an issue, a properties column taller than the tab scrolls on its own, so its last sessions are reachable without scrolling to the end of the activity.
- **Pick which pull requests notify you.** "Notify me about" is Off, Only when I'm asked to review (also when you're added to one that's already open), or Every new pull request. Background checks run every 5 minutes by default.

## 0.1.102 · 2026-10-30

- **Close a floating terminal.** A terminal floating over its agent has a Close button, on the panel and on its minimised bar.
- **Summaries while an agent works.** A session on a long turn, like a scheduled one, gets a new one-line summary in the Agents list every two minutes, not only when the turn ends.
- **Usage limits keep up.** The HUD reads your 5-hour and weekly limits every 5 seconds while Wings is in front, not once a minute.

## 0.1.101 · 2026-10-28

- **Notifications you can't miss.** While Wings is in front, a plugin's notification pops up as a toast with its buttons, like Start for a newly assigned ticket, and the Inbox count pulses and takes an accent colour until you read it.
- **Agents can propose a setup command.** An agent asks with `worktree_setup`, and Wings shows you the exact line, the project and who asked. It's saved only when you click Allow. A setup command is plain ASCII, up to 500 characters.
- **Every state in the work item tab.** The state menu lists all of the type's states, as Azure DevOps does, not just the next two.
- **No more agent todo list.** Issues drops `tasks__todo`, its card over the pane and the Todo tab. Agents wrote the list once and rarely updated it, and the issue's status and log already show progress.

## 0.1.100 · 2026-10-28

- **Delete a worktree from the sidebar.** The X on a worktree now asks: remove it from the sidebar only, or delete the worktree folder too. Its branch goes with it when it's merged, and stays, with a note, when it isn't. A worktree with uncommitted changes needs "Delete anyway".
- **Agents can read another project's issues.** Issues' list and get take a project name or folder path; creating and changing issues stays in the agent's own project.

## 0.1.99 · 2026-10-28

- **Agents can set up worktrees.** The `worktree_setup` tool and `wings worktree-setup` read a project's worktree setup and change the files copied into new worktrees, the same settings as "Worktree setup…". The setup command only you change, since it runs in every new worktree.
- **Worktree setup in Schedules.** An "assigned to me" entry shows and edits its project's worktree files and setup command.
- **Pick where agent terminals open.** Settings, Agents, "Agent terminals open": below the agent, right of it, or floating over it.
- **For plugin authors:** `wings.worktreeSetup` and `wings.setWorktreeSetup`, with `permissions.worktrees`.

## 0.1.98 · 2026-10-28

- **Merged PRs close their issues.** When a PR you track is merged, the Issues issue for it goes to done right away, matched by its PR link or its branch. An issue with only a branch now finds its PR too. Nothing outside Wings is changed.
- **Issues from a worktree go to its project.** An agent working in a worktree files its issues under the main project, where you see them.
- **Schedules opens the issue when it starts a ticket.** Start creates the ticket's Wings issue, or picks up the one that names it, and tells the agent its key.
- **Safer worktree setup.** Local files are never copied through a symlinked folder.
- **Plugins can hear each other.** For Wings' own plugins: `wings.emit` and `wings.onEvent`, with `permissions.events`.

## 0.1.97 · 2026-10-28

- **Worktrees bring your local setup.** A worktree Wings makes gets a copy of the main checkout's gitignored setup: `.claude` rules, agents, skills, hooks and commands, `.mcp.json` and `.env` files. Pick other files, or leave some out with `!`, under "Worktree setup…" in a project's right-click menu. A `.worktreeinclude` in the repo takes precedence.
- **A setup command for new worktrees.** Set one there, like `pnpm install` or a script of your own, and it runs in a new worktree before the agent starts.

## 0.1.96 · 2026-10-28

- **Pick the starting branch from a list.** In a Schedules entry, "Start branches from" lists the project's branches, and still takes one you type.

## 0.1.95 · 2026-10-28

- **Schedules fits each project.** The Schedules page shows the entries of the project you open it in, with All projects for the rest. An entry can ask only about tickets whose title starts with FE, BE or any prefixes you give it.
- **Pick the branch work starts from.** An entry's "Start branches from", like origin/develop, for repos whose default branch isn't where work starts. Agents can change an entry with `schedules__update`, or `wings schedules update` on the command line.
- **Start where you want, and again.** Start asks which project when the ticket's board feeds more than one, and a started ticket can be started again. Tickets in a state the entry doesn't ask about get Start anyway.
- **Agents a plugin started say so.** The Agents list shows a badge like "Schedules" next to an agent a plugin opened.
- **Tidier worktrees in the sidebar.** A worktree shows its branch once, and no longer repeats its project's pull requests and issues.

## 0.1.94 · 2026-10-28

- **See what's assigned to you, and start it.** A Schedules entry "When something is assigned to me" lists your tickets with their state, and you pick which states it asks about. Check now looks right away, and Start works from the list, the Inbox and the notification. A ticket that already has a branch, or a Wings issue in progress, in review or done that names it, counts as started and isn't asked about.
- **Agent terminals are easier to handle.** A terminal an agent opens below it says which agent opened it, and has buttons to hide it, move it beside or below the agent, or float it over the agent as a panel you can drag, resize and shrink to a bar.
- **Plugin tools on the command line.** `wings tools` lists them, and `wings <plugin> <tool> key=value` runs one, like `wings schedules check`.
- **Agents reuse an idle pane in another folder.** An agent's command goes to an idle agent pane of the same project, even one in a different folder, instead of opening a new one.
- **Wings' own plugins can use each other's tools.** For plugin authors: `wings.callTool` with `permissions.callTools`, only between the plugins that ship with Wings.

## 0.1.93 · 2026-10-28

- **Start on what's assigned to you.** In Schedules, add an entry "When something is assigned to me" for a project. Every 5 minutes it checks Azure DevOps, GitHub or GitLab for tickets newly assigned to you, and asks before doing anything. Start makes a worktree on a new branch for the ticket, if there's enough free disk, and opens your agent there in the background.
- **Plugins can make worktrees and open tabs in the background.** For plugin authors: `wings.createWorktree` and `openPane({ focus: false })`.

## 0.1.92 · 2026-10-28

- **Messages between agents queue up.** With "Let agents message each other" off, a message left in an agent's prompt blocked every later one until you pressed Enter or Ctrl+C there. Now they wait in the prompt together and go when you press Enter. Only text you typed yourself still stops a message.

## 0.1.91 · 2026-10-28

- **New versions stay in sight.** When an update is out, the sidebar shows "Update to Wings …" above Settings until you install it, even after you close its notice. Click it to see what's new and install.

## 0.1.90 · 2026-10-28

- **Agents don't pull you away.** When an agent opens a terminal for a command, you stay on the tab and project you're in. Its terminal starts in the background, below the agent or in its own tab, and is there when you go look.

## 0.1.89 · 2026-10-28

- **Agents take messages and go to sleep again.** Wings took the terminal's own answers to Claude Code starting up for text you had typed and not sent. So every agent looked busy with a draft: it never went to sleep, and other agents couldn't message it.
- **Git status shows for every project.** The sidebar could leave a project without its branch status until you opened it. It shows for all of them from the start now.

## 0.1.88 · 2026-10-28

- **An agent's commands run below it.** When an agent runs a build or tests in a terminal, it opens as a split below the agent's pane instead of a new tab, and your typing stays with the agent. A chip on the agent's pane, like "1 terminal", hides and shows them. They keep running while hidden, and a new command shows them again. A dev server the agent opens for you still gets its own tab.
- **PR news reaches the agent.** The PR tracker can tell the agent working on a PR's branch when its checks fail, someone leaves a review comment, or it gets merge conflicts. It checks those PRs every 2 minutes, also while Wings is in the background. Turn it on in the PR tracker's settings.

## 0.1.87 · 2026-10-28

- **Agents can message each other.** An agent can send a message to the agent in another pane, for example to hand it a task, and then wait for it to finish its turn. Plugins can do the same. Wings presses Enter for them only if you turn on "Let agents message each other" in Settings > Agents. Otherwise the message waits in that agent's prompt for you.
- **Fewer leftover tabs.** When an agent runs a command in a tab, its next command goes to the same tab once nothing runs there, instead of opening another one. Agents can also close the tabs they opened. Those tabs show a small agent mark, and agents can't close yours.
- **Schedules.** A new plugin starts an agent with a prompt by itself, every few hours or at a set time on the days you pick. Install it from Settings > Plugins. Wings has to be running, and the run opens in front of you.

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
