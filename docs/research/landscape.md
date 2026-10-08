> Research notes gathered 2026-10-08 by a research agent. Paths under `/private/tmp/...scratchpad/` were temporary and no longer exist. Treat versions and issue states as of that date.

# Landscape survey: multi-session managers for Claude Code / coding agents (as of 2026-10-08)

Basis: everything below was fetched live this session (GitHub API for stars, license, last push and release; project docs; HN via Algolia). Items marked "inferred" are my reading, not a quoted fact. Downloaded READMEs and docs sit read-only in `/private/tmp/...scratchpad/landscape/`.

## Headline findings

1. **Wrapping the real TUI in a GUI is crowded, not a gap.** Herdr, herdr-gpui, cmux, Orca, Superset, Emdash, cc-pane, claude-view, Tortie, Zed Terminal Threads and Warp all do it.
2. **The Tauri/Rust niche has a near-identical architecture already.** `Quantum-vik/claude-view` (no license, 0 stars) documents the same three layers Wings plans: a PTY mirror in xterm.js, a tailed transcript, and hooks posting to a local server. `wuxiran/cc-pane` (GPL-3.0, 567 stars) also auto-injects a built-in MCP server into every Claude launch.
3. **Per-project history with search and resume is table stakes.** Orca, opcode, cc-switch, claude-deck, Nimbalyst, Tortie, the Claude Desktop `/resume` picker and the CLI picker all have it.
4. **First-party Anthropic features now overlap Wings.** There is `claude agents` (agent view), a background supervisor with `claude agents --json`, Desktop `/resume` for CLI sessions, and "mods" (plugins that draw panes inside the TUI and register tools).
5. **The one gap still open:** a third-party plugin that ships GUI panels and MCP tools behind a single auto-registered endpoint, in a host that wraps the real TUI. Nimbalyst is the closest, but it is chat/SDK-based. Details in the last section.

## Summary table

Stars and license come from the GitHub API on 2026-10-08. "Last activity" is the latest release or push.

| name | stack | terminal approach | session tracking | plugins | license | last activity |
|---|---|---|---|---|---|---|
| **Herdr** (herdrdev/herdr) | Rust: ratatui, crossterm, portable-pty, in-repo ghostty-vt | Server-owned PTYs; TUI client; detach and reattach | Screen-manifest detection plus a Claude `SessionStart` hook that reports the session id to a socket; resume with `claude --resume <id>` | Manifest plugins (argv commands: actions, events, panes, link handlers). No native non-terminal UI in v1; no MCP anywhere in docs | Apache-2.0 | v0.9.3 2026-09-29; 42.9k stars; $6M seed Sept 2026 |
| **herdr-gpui** (penso) | Rust, GPUI | Paints the Herdr daemon's cells over a bincode client socket; no emulator of its own | Via the Herdr daemon | Shows Herdr plugin output; has browser tabs with annotations | Apache-2.0 | v20261007.3 on 2026-10-07; 1.0k stars; unaffiliated |
| **cc-pane** (wuxiran) | Tauri 2, React 19, xterm.js 6, Rust PTY crates | PTY into xterm.js | `cc-panes-cli-hook` (session_start, permission_request, notify), OSC-title capture for Codex, launch history with `resumeId` | Built-in `ccpanes` orchestrator MCP auto-injected per launch; "shared MCP" and skills; no UI plugin API found in the tree | GPL-3.0 | v0.12.20 2026-09-16; 567 stars |
| **claude-view** (Quantum-vik) | Tauri, Rust portable-pty, xterm.js | App-owned PTY mirrored in xterm.js | Transcript tail plus hooks via a bundled script posting to a local HTTP server | None | none | v1.7.1 2026-09-20; 0 stars |
| **CLI Deck** (zeroisnumber/claude-deck) | Tauri 2 (Windows) | ConPTY plus xterm.js | Scans `~/.claude/projects`; uses `~/.claude/sessions/<pid>.json` for state | None | none | v0.5.25 2026-10-07; 0 stars |
| **cc-switch** (farion1231) | Tauri / Rust | Not a terminal; macOS can resume in an external terminal | Rust `session_manager` with per-agent log parsers; search; reading view | Manages MCP, skills, prompts for tools | MIT | v4.0.4 2026-10-07; 141k stars |
| **opcode** (winfunc, ex-Claudia) | Tauri 2, React 18 | None: spawns `claude -p ... stream-json --verbose` (re-implemented chat UI) | Scans `~/.claude/projects`; own checkpoints | MCP manager UI; custom "CC Agents" | AGPL-3.0 | v0.2.0 2025-08-31; last code commit 2025-10-16 (README-only commit 2026-09-18); 22.4k stars. Effectively dormant |
| **Orca** (stablyai) | Electron, TypeScript | node-pty plus xterm.js (WebGL); optional "Chat UI" over the same PTY | Agent status hooks (OSC statusline); scans each agent's on-disk transcripts for history | None found (docs and tree grep) | MIT | v1.4.222 2026-10-07; 87.5k stars; ships daily |
| **Superset** (superset-sh) | Electron, TypeScript, Bun and Turbo monorepo | node-pty plus xterm.js with a separate `pty-daemon` package | Hooks and wrappers in each agent's config, guarded by an env var; auto-resume on crash; fork | "Plugins" are agent-side skill and MCP bundles; MCP server is cloud-hosted | Elastic License 2.0 (source-available, not OSI) | desktop-v1.37.0 2026-10-08; 15.0k stars |
| **Emdash** (generalaction) | Electron, TypeScript | node-pty plus xterm.js 6 | Marker-tagged hooks in agent configs (status, notifications, resumable sessions) | Internal issue-tracker plugins; MCP and skills manager | Apache-2.0 | v1.2.7 2026-09-27; 5.9k stars; YC W26 |
| **Nimbalyst** (ex-Crystal) | Electron, TypeScript | Chat via `@anthropic-ai/claude-agent-sdk`; separate ghostty-web plus node-pty terminal | Own sessions plus `ExternalSessionWatcher` (chokidar) importing external Claude transcripts; session kanban | Extension SDK: editors, panels, tool widgets, MCP tools | MIT | v0.79.1 2026-09-30; 1.9k stars. Crystal repo: v0.3.5 2026-02-26, 3.1k stars |
| **Parallel Code** (johannesjo) | Electron, SolidJS | Spawns the agent CLI in a worktree | Own spawning | None | MIT | v3.1.0 2026-09-26; 1.0k stars |
| **Tortie** (gregce) | Electron, xterm.js, bundled tmux | tmux-backed durable sessions | Agent logs ("Catch Me Up"); replays scrollback and prepares each agent's resume command | One-JSON-file custom agent profiles | Apache-2.0 | v0.110.0 2026-09-21; 88 stars; macOS |
| **CloudCLI / claudecodeui** (siteboon) | Node, React web | Agent SDK chat plus node-pty and xterm.js shell tab | "All existing sessions discovered automatically" (chokidar) | Plugin system: `manifest.json` with `slot: "tab"`, optional Node backend | AGPL-3.0 | v1.37.3 2026-09-08; 14.0k stars |
| **Sculptor** (imbue-ai) | Python backend, desktop shell, containers | `claude` as a streaming-JSON process with the control protocol (re-implemented UI) | Own sessions persisted | Bundled plugins for slash commands | MIT | v0.48.0 2026-09-21; 236 stars |
| **Vibe Kanban** (BloopAI) | Rust backend plus React, run via `npx` | `--output-format=stream-json` executor; xterm in the UI | Own workspaces | None | Apache-2.0 | v0.1.44 2026-04-24; 28.3k stars. **Sunsetting** (announced 2026-04-10) |
| **Conductor** (Melty Labs) | Closed, Mac only | Chat UI over a bundled `claude` binary; experimental "Big Terminal Mode" | Own workspaces; cloud API with transcript SQL | MCP config; Conductor MCP server for cloud workspaces | Proprietary | 0.90.0 on 2026-10-02 |
| **cmux** (manaflow-ai) | Swift/AppKit, libghostty | Native Ghostty-based terminal | `cmux hooks setup` saves session ids; OSC 9/99/777 notifications; socket API | Custom commands in `cmux.json`, hooks, skills | GPL-3.0-or-later (per LICENSE header) | v0.65.0 2026-10-05; 28.0k stars; macOS |
| **Claude Squad** (smtg-ai) | Go TUI | tmux; polls `capture-pane` | Own spawning | None | AGPL-3.0 | v1.0.20 2026-08-20; 8.6k stars |
| **CCManager** (kbwo) | TypeScript, Ink | node-pty plus `@xterm/headless` | PTY-output state detection; status hooks | None | MIT | v4.4.4 2026-09-27; 1.3k stars |
| **agent-of-empires** (njbrake) | Rust | tmux | Status detection | Repo hooks | MIT | pushed 2026-10-07; 3.3k stars |
| **dmux** (standardagents) | Node TUI | tmux | Own spawning | None | MIT | pushed 2026-08-16; 1.8k stars |
| **Claude Desktop, Code tab** (first-party) | Closed | Chat UI plus integrated terminal pane | Own sessions; `/resume` lists CLI sessions (Aug 2026) | Claude Code plugins, skills, connectors | Proprietary | Current |
| **Claude Code agent view and mods** (first-party) | In the CLI | The real TUI | Supervisor; `claude agents --json` | Mods: panes, bands, commands, tools | Proprietary | Current |
| **Zed** | Rust editor | ACP external agents plus "Terminal Threads" | ACP import of threads | MCP forwarded | GitHub field: NOASSERTION (mixed) | v1.23.2 2026-10-07; 91.4k stars |
| **Warp** | Rust | Native terminal | `claude-code-warp` plugin hooks emit OSC 777 to `warp://cli-agent` | Claude Code plugin (MIT) | AGPL-3.0 | pushed 2026-10-08; 65.4k stars |
| **Cursor 3 Agents Window; Windsurf, now Devin Desktop** | Closed | Chat | Own | Marketplace, MCP | Proprietary | Cursor 3.0 on 2026-04-02 |

## Per-app notes

### Closest to Wings

- **Herdr** https://github.com/herdrdev/herdr (https://herdr.dev/docs/)
  - What it is: a terminal multiplexer for agents. A background server owns the PTYs and a TUI attaches. It reports idle/working/blocked per pane, rolled up to tab and workspace.
  - Sources: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.3/docs/next/website/src/content/docs/agents.mdx, .../integrations.mdx, .../session-state.mdx, .../plugins.mdx.
  - State detection: screen-manifest rules read the live pane bottom. Remote manifest updates apply without a restart, and local overrides live in `~/.config/herdr/agent-detection/<agent>.toml`. The Claude integration is a `SessionStart` hook (matcher: startup, resume, clear, compact, fork) that reports the session id to the local socket. It writes `hooks/herdr-agent-state.sh` and edits `settings.json`.
  - Resume: `claude --resume <id>` after a server restart.
  - Plugins: "Runtime action registration and native non-terminal plugin UI are not part of plugin v1." `rg -i mcp` over all its docs returns nothing.
  - Wings overlap: Herdr already auto-discovers a `claude` the user types in a pane.
  - Business context: https://herdr.dev/blog/herdr-raised-a-seed/ (Bessemer-led seed). The runtime is stated to stay open: https://herdr.dev/blog/herdr-is-joining-y-combinator/
  - Reuse: Apache-2.0 Rust (portable-pty, `crates/ghostty-vt`, a socket API with `events.subscribe`).
- **herdr-gpui** https://github.com/penso/herdr-gpui
  - This is a GUI for Herdr, not an alternative to it. A native Rust/GPUI client paints the daemon's terminal cells without running another emulator.
  - Features: worktree "Teleport" (moves transcripts to another host and resumes), diff review with notes sent back to the agent, browser tabs with annotations, status dots, Dock badge.
  - Its own list of limitations: no session picker, no notification history, no draggable scrollback.
  - Related community clients: `powerfooI/herdr-studio` (now `roamgate`, 282 stars, MIT), `AltanS/collie` (mobile PWA). HN reaction: https://news.ycombinator.com/item?id=49652188
- **cc-pane** https://github.com/wuxiran/cc-pane (Tauri 2, GPL-3.0)
  - Closest to the "single auto-registered MCP" idea. Its doc says (translated) "you don't need to configure it, it's already there": https://github.com/wuxiran/cc-pane/blob/main/docs/guide/mcp-orchestration.md
  - Mechanism: `cc-cli-adapters/src/claude.rs` builds a per-launch `--mcp-config` containing a `ccpanes` server (highest priority over user entries). Other CLIs get a project file written with a receipt-based ownership record (`cc-cli-adapters/src/mcp_file_injection.rs`).
  - Tools: dozens, covering launching sessions, reading another session's output, resuming history by `resumeId`, leader/worker orchestration, memory and todos.
  - Also has `cc-panes-cli-hook`, a `cc-panes-daemon`, and web and mobile crates.
  - Plugins: no third-party UI plugin API in the file tree; extensibility is skills and shared MCP.
  - Reuse: ideas only, because of GPL-3.0.
- **claude-view** https://github.com/Quantum-vik/claude-view
  - Its README states the three-layer design explicitly. It also notes that none of Claude Code's channels (hooks, SDK, stream-json, transcripts, OTel) stream tool output live, so the PTY mirror is the only live view.
  - Layers: live terminal via `portable-pty` plus xterm.js; trace and cost from the tailed transcript (about 0.2 s behind); session state from hooks posting to a local HTTP server.
  - Sessions launch with `--dangerously-skip-permissions` by default (a toggle exists).
  - No license, so no code reuse.
- **CLI Deck** https://github.com/zeroisnumber/claude-deck
  - Session sidebar from `~/.claude/projects`; click to run `claude --resume` in an embedded ConPTY; `--fork-session` copy; token breakdown.
  - `src-tauri/src/activity.rs` prefers `~/.claude/sessions/<pid>.json`, falling back to an output-density guess. I grepped the full Claude Code docs for `sessions/<pid>` and found nothing, so that file looks undocumented.
- **cc-switch** https://github.com/farion1231/cc-switch (Tauri/Rust, MIT)
  - Provider switching, centralised MCP and skills sync, and a Sessions panel (browse, search, reading view, copy resume command).
  - Reuse candidate: `src-tauri/src/session_manager/providers/{claude,codex,gemini,...}.rs`, which are Rust parsers for each agent's logs.
- **opcode** https://github.com/winfunc/opcode
  - `src-tauri/src/commands/claude.rs` scans `~/.claude/projects` and spawns `claude -p ... stream-json --verbose` with `--resume`. There is no PTY or xterm.
  - It is not the same product shape as Wings: a re-implemented chat UI, and dormant since 2025-10.

### Electron and web apps

- **Orca** https://github.com/stablyai/orca
  - Agent Session History panel: https://github.com/stablyai/orca/blob/main/docs/site/content/docs/agents/session-history.mdx. It scans transcripts for 18 agents, offers scope Workspace/Project/All, searches by title/cwd/branch/model/preview, and resumes with `claude --resume <id>` in a new terminal.
  - Native chat doc: https://github.com/stablyai/orca/blob/main/docs/site/content/docs/agents/native-chat.mdx. Chat UI is "a structured transcript + composer for the same PTY" and the terminal stays the source of truth. It is experimental and decodes Claude, Codex, Grok and OMP. This is the closest existing product to "live UI from transcripts over the real TUI".
  - Hooks doc: https://github.com/stablyai/orca/blob/main/docs/site/content/docs/agents/hooks-memory.mdx. Hook endpoints are written to disk and re-sourced, so long-lived sessions survive an app restart.
  - It also has an Orca CLI, a mobile companion and a Design Mode browser.
- **Superset** https://github.com/superset-sh/superset (https://docs.superset.sh/llms.txt)
  - Status doc (https://docs.superset.sh/llms.mdx/agent-status): hooks and wrappers go into each agent's config, are guarded by an env var, and do nothing outside Superset terminals. Claude reports both "finished" and "waiting".
  - Sessions doc (https://docs.superset.sh/llms.mdx/agent-sessions): auto-resume after a terminal dies, a "Resuming..." pill, fork via `claude --resume <id> --fork-session`, and "Continue with another agent" seeded with context.
  - MCP doc (https://docs.superset.sh/llms.mdx/mcp-server): hosted at `https://api.superset.sh/mcp`, added by hand with `claude mcp add`. It manages tasks, workspaces and terminals.
  - Its `plugins/` directory holds first-party skill and MCP bundles listed in `.agent-marketplace.json`.
  - Elastic License 2.0 (https://github.com/superset-sh/superset/blob/main/LICENSE.md), so reuse is restricted.
- **Emdash** https://github.com/generalaction/emdash. README: hooks "track status, notifications, and resumable sessions, and silently do nothing when the agent runs outside an Emdash session". It also has SSH projects and 12 ticket integrations.
- **Nimbalyst / Crystal** https://github.com/nimbalyst/nimbalyst (Crystal https://github.com/stravu/crystal now redirects to it)
  - Extension architecture: https://github.com/nimbalyst/nimbalyst/blob/main/docs/EXTENSION_ARCHITECTURE.md. Extensions contribute editors, file handlers, "AI Tools via MCP" and panels.
  - MCP topology: https://github.com/nimbalyst/nimbalyst/blob/main/docs/INTERNAL_MCP_SERVERS.md. A single unified internal HTTP server on one port with one bearer token hosts every first-party endpoint, and each extension gets `/mcp/ext/<id>` (deferred-loaded by ToolSearch).
  - Agent path: `ClaudeCodeProvider` uses the Agent SDK `query()` (chat), not the TUI. The terminal is a separate ghostty-web plus node-pty panel.
  - `packages/electron/src/main/services/externalSessionWatcher/ExternalSessionWatcher.ts` (chokidar, bounded discovery) ingests Claude sessions started elsewhere.
- **CloudCLI / claudecodeui** https://github.com/siteboon/claudecodeui. Plugin starter: https://github.com/cloudcli-ai/cloudcli-plugin-starter (`manifest.json` has `slot: "tab"`, `entry`, `server`). Docs: https://cloudcli.ai/docs/plugin-overview. This is a UI plugin system without MCP injection.
- **Sculptor** https://github.com/imbue-ai/sculptor. Integrated-harness doc: https://github.com/imbue-ai/sculptor/blob/main/docs/help/integrated_harnesses.md ("runs Claude Code as a streaming JSON process with its control protocol"; it replaces AskUserQuestion and ExitPlanMode with its own tools). Imbue also ships `mngr` (https://github.com/imbue-ai/mngr), a tmux/SSH CLI with plugins.
- **Vibe Kanban** https://github.com/BloopAI/vibe-kanban. Shutdown notice: https://www.vibekanban.com/blog/shutdown (company shutdown 2026-04-10; repo continues community-maintained; remote services removed).
- **Conductor** https://www.conductor.build/llms.txt. Big Terminal Mode (https://www.conductor.build/docs/reference/big-terminal-mode) is experimental and its sessions restore after restart. Primary UI is chat over a bundled Claude Code binary. Changelog: https://www.conductor.build/changelog.md
- **Tortie** https://github.com/gregce/tortie. Sessions are durable, run outside the app window and survive restarts; "Catch Me Up" reads each session's own log.
- **Parallel Code** https://github.com/johannesjo/parallel-code. Worktrees, a diff viewer with inline comments, and a Steps panel that writes `.claude/steps.json`.
- **TOKENICODE** https://github.com/yiliqi78/TOKENICODE (Tauri 2, Apache-2.0, 445 stars, last push 2026-06-17). A chat-style GUI using the "SDK Control Protocol" (per its README).
- **Terragon** https://github.com/terragon-labs/terragon-oss. Defunct: the README says "snapshot ... at the time of shutdown (January 16, 2026)". It was a cloud agent orchestrator with a `terry` CLI and an MCP server.
- **Claudable** https://github.com/anymorph-ai/Claudable is a web app-builder, not a session manager. Ignore.

### Terminal-native and native apps

- **cmux** https://github.com/manaflow-ai/cmux (https://cmux.com/llms.txt). Session restore doc: hooks save a native session id so supported agents resume. Notifications come from OSC 9/99/777 and `cmux notify`. A socket API and an in-app scriptable browser.
- **Claude Squad** https://github.com/smtg-ai/claude-squad. `session/tmux/tmux.go` polls `capture-pane` content for updates and auto-dismisses the trust prompt.
- **CCManager** https://github.com/kbwo/ccmanager. Per-CLI state-detection strategies; status-change hooks; copies Claude session data between worktrees.
- **Other tmux or TUI tools:** `njbrake/agent-of-empires` (Rust, MIT), `standardagents/dmux`, `leapmux/leapmux` (Go plus SolidJS plus Tauri shell, FSL-1.1-ALv2). `coder/mux` (renamed Xum, AGPL-3.0) is a multi-model harness of its own, not a wrapper.
- **Transcript-driven read-only views:** `furkankly/zoetrope` (Rust, ratatui, MIT, 1.0k stars; live and replay flow graph from Claude and Codex transcripts), and Show HN tools such as claude-replay and Claudetop.
- **Nicsilver/claude-sessions** https://github.com/Nicsilver/claude-sessions (Tauri, AGPL-3.0). It installs global hooks in `~/.claude/settings.json`, so it tracks every session from any terminal or IDE: needs-you, your-turn, working, done.

### First-party and editors

- **Claude Desktop, Code tab** https://code.claude.com/docs/en/desktop.md
  - Panes: chat, diff, terminal, browser. Parallel sessions with worktrees. It installs plugins.
  - "Each keeps its own session list, and you can bring a CLI session into Desktop" via `/desktop`.
  - Desktop `/resume` lists terminal sessions with search by title, folder or branch: https://code.claude.com/docs/en/whats-new/2026-w35.md
  - Claude "sees only the sessions the desktop app runs itself".
- **Claude Code agent view** https://code.claude.com/docs/en/agent-view.md
  - `claude agents` is a TUI dashboard of background sessions run by a supervisor service. State is `working`, `blocked`, `done`, `failed` or `stopped`.
  - `claude agents --json` is "the supported way to read session state from outside Claude Code". The files under `~/.claude/jobs/` are explicitly "not a stable interface".
- **Claude Code mods** https://code.claude.com/docs/en/plugins/mods/overview.md and https://code.claude.com/docs/en/plugins/mods/api.md
  - Mods are plugins whose JS handlers draw panes, bands, buttons and text fields in the TUI. `$.tool.register` exposes a tool to Claude as `mcp__<plugin>__<tool>` and `$.command.register` adds a slash command.
  - Mods run in the CLI and the Desktop Code tab; panes draw only in those two.
  - Consequence: Wings embedding the real TUI gets mod UI drawn inside it for free, and also competes with it.
- **Zed** https://zed.dev/docs/ai/external-agents (Claude via ACP, with Import Threads from agents) and https://zed.dev/docs/ai/terminal-threads. Terminal Threads run the native CLI or TUI in a terminal that Zed groups by project in the Threads Sidebar.
- **Warp** https://docs.warp.dev/agent-platform/third-party-agents/claude-code and https://github.com/warpdotdev/claude-code-warp. Claude hook scripts emit OSC 777 JSON to `warp://cli-agent`, with protocol version negotiation. This is the in-band channel idea (no ports).
- **Cursor 3 Agents Window** https://cursor.com/changelog/3-0. **Windsurf, now Devin Desktop**, Agent Command Center (kanban): https://docs.windsurf.com/windsurf/agent-command-center. Closed products that do not wrap the Claude TUI; low relevance.
- **Superlogical "Rex"** https://www.superlogical.com. A multiplexer for all work (native macOS/iOS and web), creator listed as Mitchell Hashimoto; beta waitlist only. I read the marketing page only.
- Secondary source for rankings: https://github.com/pinion05/awesome-agent-ide (a July 2026 list; its numbers are stale).

## What Wings can reuse

- **Permissive Rust or TS code:**
  - Herdr (Apache-2.0): vendored portable-pty, `ghostty-vt`, socket and event API design.
  - cc-switch (MIT): per-agent log parsers in `session_manager/providers`.
  - Nimbalyst (MIT): `ExternalSessionWatcher` and the extension and MCP topology docs.
  - Orca (MIT): transcript scanners for 18 agents.
  - Emdash (Apache-2.0): marker-tagged hook install and uninstall.
- **Ideas only:** cc-pane (GPL-3.0), opcode, Claude Squad, CloudCLI (AGPL-3.0), cmux (GPL), Warp (AGPL), Superset (ELv2). No license: claude-view, CLI Deck.
- **Design ideas worth copying:**
  - Three-layer observation: PTY mirror plus tailed transcript plus hooks (claude-view).
  - Hooks guarded by an env var so they no-op outside the app, and marked or receipted entries so uninstall removes only the app's own (Superset, Emdash, cc-pane).
  - An in-band OSC channel from hooks to the host (Warp, cmux, Orca statusline).
  - `claude agents --json` instead of undocumented files.
  - Status rollup pane to tab to workspace (Herdr).
  - Remotely updatable detection manifests (Herdr).
  - Auto-resume with a "Resuming..." pill (Superset), `--fork-session` (Superset, CLI Deck), "Catch Me Up" (Tortie), history scope toggle (Orca).

## Gaps none of them fill

1. **Plugin system with UI plus MCP tools via one auto-registered endpoint.** This is the real gap, but it is narrow.
   - Nimbalyst is nearest: extensions bundle UI and MCP tools behind one localhost server with one bearer token, using `/mcp/ext/<id>` per extension. It drives Claude through the Agent SDK (chat), and extension UI centres on file editors, so it does not wrap the TUI.
   - cc-pane auto-injects one `ccpanes` MCP per launch, but it is first-party and not an extension point.
   - Herdr plugins are argv commands and terminal panes, with no native UI and no MCP.
   - CloudCLI plugins are UI tabs with a Node backend and no MCP.
   - Superset plugins are agent-side skill and MCP bundles, and its MCP is hosted and added by hand.
   - Claude Code mods overlap strongly: in-process UI plus tools named `mcp__<plugin>__<tool>`. They only draw inside the TUI and Desktop, and they are Claude-only.
   - Implication for Wings: the defensible slice is GUI panels outside the TUI, plus MCP tools usable by non-Claude agents, plus one shared endpoint and registration. Mods can already cover panes inside the TUI.
2. **Wrapping the real interactive TUI is not a gap.** It is done by Herdr(+gpui), cmux, Orca, Superset, Emdash, cc-pane, claude-view, Tortie, Zed, Warp. Re-implemented chat UIs are opcode, Sculptor, Vibe Kanban, Nimbalyst, CloudCLI, Conductor and the Desktop Code tab. Only the Tauri/Rust combination is thin: cc-pane (GPL), claude-view and CLI Deck (unlicensed, tiny).
3. **Auto-discovering sessions the user starts in the app's terminal is not a gap.** Herdr (screen detection plus `SessionStart` hook), cmux, Superset and Emdash (env-guarded hooks), Orca, cc-pane and Nicsilver (global hooks) all do it. Superset and Orca typing-in-terminal behaviour is inferred from their docs, not tested.
4. **Structured live UI driven by transcripts, beside the real TUI, in a host with history and plugins, is only partly filled.** Orca's Chat UI is experimental. claude-view is one window per session with no plugin story. zoetrope is read-only. cc-switch and Orca cover history but not live panes.
5. **Convergence risk, not a gap:**
   - Anthropic ships `claude agents`, a supervisor, Desktop `/resume` for CLI sessions and mods.
   - Herdr has $6M of funding and the GUI client already exists (herdr-gpui).
   - A Wings pitch of "GUI Herdr" overlaps herdr-gpui directly (Rust, Apache-2.0, 1.0k stars).

Unverified: Superlogical's product details beyond its landing page; Zed's exact license split (GitHub says NOASSERTION); whether Orca has any plugin system (I found none in its docs or tree, which is absence of evidence).