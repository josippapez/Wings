> Research notes gathered 2026-10-08 by a research agent. Paths under `/private/tmp/...scratchpad/` were temporary and no longer exist. Treat versions and issue states as of that date.

# Herdr feature and architecture inventory (v0.9.3)

## Sources and basis
- Source: https://github.com/herdrdev/herdr, tag v0.9.3 (commit 7b116c05bfda646af39d2524c54e70c751f57ee8, 2026-09-29). It is cloned read-only at `/private/tmp/...scratchpad/herdr-src/repo`. Below, `src/...` and `docs/...` are relative to that root.
- Repo found via `brew info herdr` and the Homebrew formula. The formula builds Rust plus Zig 0.16 (`vendor/libghostty-vt`) and is Apache-2.0.
- Docs pages: `docs/next/website/src/content/docs/<page>.mdx` is what https://herdr.dev/docs/<page>/ renders. https://herdr.dev/llms.txt says its links serve the raw files from the documented tag. I read these locally. I also fetched the live homepage, /docs/, /plugins/, /compare/, /cloud/, and the blog pages. Copies are in `.../scratchpad/herdr-web/`.
- Plugin data comes from the marketplace feeds https://assets.herdr.dev/plugins/index.json and https://assets.herdr.dev/stats/stats.json.
- Measured locally: `herdr --help`, `herdr <sub> --help`, `herdr integration status`, `herdr status`, `herdr agent list`, `herdr agent explain $HERDR_PANE_ID --json`, `~/.config/herdr/session.json` (key names only), and `~/.claude/settings.json`. All were read-only.
- Caveat: the v0.9.3 docs say resume commands need "Herdr 0.10.0 or later" (`add-herdr-support.mdx`). The code (`src/agent_resume.rs`, `ReportedAgentResume`) is already in the 0.9.3 tag. I did not test whether a 0.9.3 server accepts `resume_argv` (unverified).
- Size: about 270k lines of Rust in `src/` (measured with `wc`).

## 1. Feature inventory
Each bullet gives its source.
- **Hierarchy** (`docs/.../concepts.mdx`):
  - Workspace is the project container, shown as a "space" in the sidebar. Tab is a layout inside a workspace. Pane is a real terminal. Agent is a recognized process in a pane.
  - Sidebar state rolls up from agents to pane, tab and workspace. Blocked beats working; done stays visible until you view it (`agents.mdx`, "State rollups").
  - Default sidebar rows for spaces are `[state_icon, workspace]` and `[branch, git_status]` (`configuration.mdx:362+`). This matches your screenshot (branch under each space, status dot).
- **Agent panel**:
  - Two sort modes: `ui.agent_panel_sort = "spaces" | "priority"` (yours is "spaces"). Default agent rows are `[state_icon, machine, workspace, tab]` and `[agent]`.
  - Row tokens are `state_icon`, `state_text`, `machine`, `workspace`, `tab`, `pane`, `agent`, `terminal_title`, `terminal_title_stripped`, and custom `$name`.
  - Tokens take conditional styling rules (equals, contains, gt, lt, hide). `rows_by_agent` overrides per agent. `ui.status_indicators` is `dots` or `symbols`.
  - Plugins can install a declarative filter and sort projection with `agent.view.set` (`socket-api.mdx:368`).
- **Agent states**: `blocked`, `working`, `done` (finished but unseen), `idle` (finished and seen), `unknown` (`concepts.mdx`).
  - Each client tracks its own "seen" for Done badges. The server also keeps a seen state used by the CLI/API. `completion_seq` identifies completed work (`agent-automation.mdx`).
- **Panes**:
  - Split right/down, zoom, swap, move across tabs and workspaces, resize mode, directional focus, copy mode, edit scrollback, rename, and mouse drag of borders. Defaults are in the `keys.*` config table.
  - Layout export and apply: `layout.export` and `layout.apply` use a BSP tree (`socket-api.mdx:177+`).
  - Kitty graphics pass through and are on by default (`CHANGELOG.md` 0.9.0).
- **Keybindings**: tmux-style prefix `ctrl+b` (`keys.prefix`) plus mouse as first-class.
  - Defaults: `prefix+c` new tab, `v` and `minus` split, `h/j/k/l` focus, `w` workspace picker, `g` goto, `z` zoom, `q` detach, `[` copy mode, `?` help, `s` settings, `b` toggle sidebar.
  - There are custom command bindings, indexed jumps, and a `plugin_action` binding type (`keyboard.mdx`, `configuration.mdx:124-250`, `plugins.mdx`).
- **Themes**: built-ins include catppuccin, tokyo-night(-day), and gruvbox(-light). There is also `terminal`, which follows the host ANSI palette.
  - `theme.auto_switch` follows host light/dark. `theme.custom.*` has about 20 color slots, each with separate light and dark variants (`configuration.mdx:249+`, `data/config-reference.json`).
- **Notifications**:
  - `ui.toast.delivery = off | herdr | terminal | system` (yours is `herdr`). Popups are suppressed for the active tab.
  - `terminal` uses the outer terminal's notifications (Ghostty, iTerm2, Kitty, WezTerm; `src/terminal_notify.rs:1-30`). `system` uses `terminal-notifier` then `osascript` on macOS.
  - Sound is mp3 with per-agent overrides (`ui.sound.*`).
  - `herdr notification show` and the `notification.show` method exist. `keys.open_notification_target` is bound to `prefix+o`.
- **Session persistence**: see section 3.
- **Worktrees**: `herdr worktree list|create|open|remove` and sidebar "New worktree". Checkouts go under `worktrees.directory` (default `~/.herdr/worktrees/<repo>/<branch-slug>`) and group under the parent workspace (`configuration.mdx:96`). Worktree events: `worktree.created`, `worktree.opened`, `worktree.removed`.
- **Named sessions**: `herdr --session <name>` and `herdr session list|attach|stop|delete`. Each has its own sockets and state, but all share one config file (`persistence-remote.mdx`).
- **Multiple clients**: several TUI clients can view different workspaces or tabs of one server independently (`concepts.mdx`, 0.9.0).
- **Remote and SSH** (`connecting-machines.mdx`, `persistence-remote.mdx`):
  - `herdr --remote <ssh-target>` gives a local UI attached to a remote server over a bridge.
  - `herdr machine add|list|status|reconnect|rename|enable|disable|remove` keeps Local plus saved SSH machines in one TUI. Machines appear in the sidebar with independent reconnects, and each machine keeps its own server.
  - Auth is plain OpenSSH. Profiles store only id, label, target, session, enabled.
  - `herdr --machine <label> <cmd>` forwards CLI commands to a remote server.
  - Plain `ssh host` then `herdr` also works, including from a phone.
  - Linux, macOS and Windows clients are supported. Windows hosts are x86_64 only.
- **Direct attach**: `herdr agent attach <name>` and `herdr terminal attach <id>` attach your current terminal to one pane (Unix only). `herdr terminal session observe|control` emit NDJSON `terminal.frame` records (base64 ANSI) for third-party bridges.
- **Supported agents** (`agents.mdx`; `src/detect/mod.rs:49-75` `Agent` enum, 24 variants):
  - With screen detection plus integration: Claude Code, Codex, Copilot CLI, Cursor Agent, OpenCode, Pi, OMP, Droid, Devin, Kimi, Kilo, Hermes, Qoder, Qwen, Letta, MastraCode, Grok, Antigravity.
  - State only: Amp, Kiro, Maki, Gemini (less tested), Cline (less tested), Muse.
  - Self-reporting agents: Crush, Command Code, Muse, Prime Agent. Anything else is shown as a plain terminal.
- **Agent control via CLI/API**:
  - `agent start --kind <k>`, `agent prompt [--wait]`, `agent wait --until`, `agent read --source recent|recent-unwrapped|visible|detection`, `agent send-keys`, `agent rename`, `agent focus`, `agent explain`.
  - `pane run|send-text|send-keys|wait-output|read|split|swap|move|zoom|layout|neighbor|edges|process-info`.
  - Alternate-screen history reads: Claude Code's full-screen transcript is read by synthesizing mouse-wheel scrolls (`agent-automation.mdx`, "Alternate-screen history reads"; `src/server/alt_screen_read.rs`).
- **Agent skill**: `herdr --skill` prints `skills/herdr/SKILL.md`, which is installable as a Claude skill (`agent-skill.mdx`).
- **Other**:
  - Responsive mobile layout (`ui.mobile_width_threshold = 64`), outer window title template, IME handling, image clipboard bridging for remote, link handling with Ctrl-click and OSC 8, and an in-app settings overlay with an integrations tab.
  - Updater: `herdr update` (not under Homebrew), channels stable/preview, and experimental live handoff. Remote detection manifests are fetched from herdr.dev (`[update] manifest_check`).
- **Config** (`~/.config/herdr/config.toml`, canonical keys in `docs/next/website/src/data/config-reference.json`):
  - Yours is tiny: `ui.toast.delivery="herdr"`, `ui.agent_panel_sort="spaces"`, `theme.name="catppuccin"`, `experimental.pane_history=true`.
  - Defaults worth copying: `ui.sidebar_width=26` (min 18, max 36), `advanced.scrollback_limit_bytes=10000000`, `session.resume_agents_on_restore=true`, `session.startup_per_agent_delay_ms=100`.

## 2. Architecture
- **Language and stack** (`Cargo.toml`): Rust 2021, a single binary called `herdr`.
  - Dependencies: ratatui 0.30, crossterm 0.29, tokio, bincode 2, serde_json, `interprocess` 2.4 (local sockets and Windows named pipes), `portable-pty =0.9.0` (patched, vendored in `vendor/portable-pty`), clap, toml, regex.
  - macOS and Linux are first-class, and there is a native Windows beta.
- **Terminal emulation** is `libghostty-vt`, Ghostty's terminal-state library written in Zig and MIT-licensed.
  - It is vendored in `vendor/libghostty-vt`, wrapped by the `crates/ghostty-vt` crate (`bindings.rs`, `lib.rs`), and built with Zig 0.16 via `build.rs`. It is not alacritty_terminal, vt100, or wezterm-term.
  - The live handoff blog post confirms it: "Herdr uses libghostty-vt for pane terminal state".
- **Client/server split**:
  - `herdr` auto-launches or attaches to a headless server (`src/server/`, `src/server/autodetect.rs`; `herdr server` runs headless).
  - Local sockets in the data dir (yours is `~/.config/herdr/`): `herdr.sock` is the JSON API and `herdr-client.sock` is the TUI client endpoint (`src/session.rs:169-184`). Named sessions live in `sessions/<name>/`.
  - API transport is newline-delimited JSON (`{"id","method","params"}`) over a Unix socket or named pipe (`socket-api.mdx:606`).
  - The TUI endpoint speaks bincode and a JSON handshake (`src/protocol/wire.rs`, `src/protocol/endpoint/`) with surface delta streaming (`src/protocol/surface_delta*.rs`, `src/protocol/render_ansi.rs`).
  - Since 0.9.0 the outer UI (sidebar, menus, themes, copy mode) renders in each client. Servers only own sessions and supply per-pane "surfaces" (`CHANGELOG.md` 0.9.0, "The terminal UI now runs in each client"; blog "Connecting the machines"). This is what lets one client federate several servers.
  - Endpoint generation 1 is a frozen compatibility contract: `AGENTS.md` "Stable client endpoint contract". Check with `herdr status`; here it shows protocol 22, endpoint_protocol_generation 1.
- **PTY handling**:
  - The server owns the PTY master. Code is in `src/pty/` (`actor.rs`, `actor/unix.rs`, `backend/`) and `src/pane.rs` (reader/writer tasks).
  - Live handoff transfers PTY master fds to a new server over `SCM_RIGHTS` (blog "Live updates without killing your terminal processes"; `src/server/handoff.rs`).
  - Hidden panes still parse bytes but skip rendering (blog "Ten agents, three clients").
- **State vs runtime separation**: `AppState` is pure serializable data and `PaneRuntime` is separate (`AGENTS.md` Principles). The CLI is a thin wrapper over the socket API (`src/cli/*`). `herdr api schema --json` prints the JSON Schema (protocol 22, schema_version 1).
- **Platform code** is isolated in `src/platform/{macos,linux,windows}.rs`. Process inspection uses libproc and `sysctl` on macOS.
- **Raw API method areas** (`socket-api.mdx:93-120`): server, notification, client window title, `session.snapshot`, workspace, worktree, tab, pane, popup, layout, agent, `events.subscribe/wait`, integration install/uninstall, plugin.*.
  - Events: `workspace.*`, `tab.*`, `pane.*` (including `pane.agent_status_changed` and `pane.output_matched`), `layout.updated`, `worktree.*`. Event history is not durable; a slow subscriber gets `events_lost` and must resync with `session.snapshot`.
- **Pane env** seen by every process in a pane (verified in this shell): `HERDR_ENV=1`, `HERDR_PANE_ID` (e.g. `wM:p1`), `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`, `HERDR_SOCKET_PATH`, `HERDR_BIN_PATH`.

## 3. Persistence and restore, and Claude session IDs
- **Live persistence**: detach (`prefix+q`) leaves the server and all processes running. This is the strongest path (`session-state.mdx`).
- **Snapshot restore** after server restart restores workspaces, tabs, panes, cwd, layout and focus. Processes are gone; panes come back as new shells in the saved cwd.
  - A pane with a missing cwd stays in the layout with an error.
  - Format: `session.json` is `SessionSnapshot` version 3 (`src/persist/snapshot.rs:10-135`). Fields: `workspaces[]`, `active`, `selected`, `sidebar_width`, `sidebar_section_split`, `collapsed_space_keys`.
  - Workspace fields: `id`, `custom_name`, `identity_cwd`, `worktree_space`, `public_pane_numbers`, `public_tab_numbers`, `tabs[]`, `active_tab`.
  - Tab: `layout` (BSP of `{Pane:id}` / `{Split:{ratio,first,second}}`), `panes{}`, `zoomed`, `focused`, `root_pane`.
  - Pane: `cwd`, `label`, `agent_name`, `managed_agent_kind`, `agent_session{source,agent,kind(id|path),value}`, `agent_resume{source,agent,argv}`, `launch_argv`.
  - Writer: `src/persist/writer.rs`; restore: `src/persist/restore.rs`.
- **Snapshots and recovery**: up to 48 layout snapshots in `session-snapshots/`, at most one per 15 minutes. Failed-load copies go to `session-backups/` (keeps 3). Nothing is restored automatically (`session-state.mdx`).
- **Pane screen history** (`experimental.pane_history`, off by default; you have it on):
  - It writes `session-history.json` next to `session.json` with `{version, layout_fingerprint, workspaces[].tabs[].panes{id:{ansi, lines}}}` (`snapshot.rs:32-50,132`). That is the ANSI scrollback you flagged.
  - It is replayed only if the `layout_fingerprint` matches exactly. It is skipped for panes that get a native agent resume.
- **Native agent resume** (default on): the integration reports a session ref per pane; after a server restart Herdr types the resume command into the restored shell.
  - Claude: `claude --resume <session_id>` (`src/agent_resume.rs:205-211`).
  - Codex: `codex resume <id>`. Also supported: Copilot, Devin, Droid, Kimi, Qoder, Qwen, Letta, OpenCode, Kilo, Hermes, Mastra, Cursor, Grok, Antigravity, Pi, OMP (`session-state.mdx` table).
  - It runs for all eligible panes across workspaces after a client attaches with terminal size and theme (`session-state.mdx`; `src/app/agent_resume.rs`). Spacing is `session.startup_per_agent_delay_ms`.
  - Custom agents can report `resume_argv` through `pane report-agent` (`add-herdr-support.mdx`). Rules: bare command name, at most 64 args and 8 KiB, no apostrophes or control characters.
- **How Herdr learns the Claude session id**: the Claude hook script `src/integration/assets/claude/herdr-agent-state.sh` is registered for `SessionStart`.
  - It reads the hook JSON from stdin (`session_id`, `transcript_path`, `source`). It skips subagents (`agent_id` present) and Cursor.
  - It sends the socket request `pane.report_agent_session` with `source "herdr:claude"`, `agent "claude"`, `agent_session_id`, `agent_session_path` (the transcript path), `session_start_source`, and a `seq` from the clock.
  - The script uses `python3` and `HERDR_ENV/SOCKET_PATH/PANE_ID`, and exits silently outside Herdr.
  - The hook matcher is `^(startup|resume|clear|compact|fork)$` (`src/integration/claude_settings.rs:17-19`).
  - Older Herdr versions also installed PostToolUse, UserPromptSubmit, Stop, PermissionRequest and SessionEnd hooks that reported working/idle/blocked. v10 only keeps SessionStart and removes the others on install (`claude_settings.rs:26-56`, `HOOK_REMOVALS`). So Herdr tried hook-driven state for Claude and moved to screen detection.
  - Herdr does not read `~/.claude/projects` transcripts for state. It only stores the transcript path (unverified whether it reads it elsewhere; I found no such use in the detection path).
- **Observed on this machine (measured)**:
  - `herdr integration status` says `claude: current (v10)` and the script exists at `~/.claude/hooks/herdr-agent-state.sh`. But `~/.claude/settings.json` has no herdr hook entry (only `PreToolUse` and `PostToolUse` keys; `rg herdr` over `settings.json` and `settings.local.json` matched nothing). No pane in your `session.json` has `agent_session` or `agent_resume`. So Claude session ids are not being captured here and restart-resume would not work.
  - "Current" is a false positive. `integration_state_for_path` (`src/integration/registry.rs:446-470`) only checks that the script file exists and its `HERDR_INTEGRATION_VERSION` header is at least the expected version; it never checks the settings entry.
  - Why the entry is missing is unverified. Another tool may have rewritten `settings.json`, or it may be an install-order issue.
  - Codex integration is not installed either. Only pi, claude and copilot scripts are present.

## 4. Detection mechanism
Two independent steps: find the agent process, then classify state from the screen. For Claude Code, state comes only from the screen, plus the OSC title.

**Step A: is this pane running Claude Code?** (process-tree inspection, no log or transcript reading)
- Each pane has a tokio task `spawn_basic_detection_task` (`src/pane.rs:792`). It ticks every 300 ms (`pane.rs:836`), or every 100 ms while confirming a working-to-idle change.
- `crate::detect::foreground_job(shell_pid)` calls `platform::foreground_job` (`src/platform/macos.rs:421`).
  - It reads the PTY foreground process group id from the pane shell's `proc_bsdinfo.e_tpgid` via `proc_pidinfo(PROC_PIDTBSDINFO)` (`macos.rs:511`). The cheap per-tick probe compares that pgid to the last one (`pane.rs:~900`).
  - It lists the group's pids with `proc_listpids(PROC_PGRP_ONLY)` (`macos.rs:487`).
  - It reads each process's name and argv with `sysctl KERN_PROCARGS2` (`macos.rs:543-640`). Linux uses `/proc` equivalents.
- `identify_agent_in_job` (`src/detect/mod.rs:249`) maps each process to an agent:
  - It uses argv0 or the process name via `lookup_agent` (`detect/mod.rs:198`): `"claude" | "claude-code"` maps to `Agent::Claude`.
  - It handles wrappers: node, bun, python, sh/bash/zsh script args, tmux (explicitly not followed), nix wrappers, Windows cmd and powershell.
  - It prefers the process-group leader (`detect/mod.rs:~255-275`).
- Escape hatch: `HERDR_AGENT=<agent>` in a process's environment tells Herdr which manifest to use (`platform/macos.rs:948`, `process_agent_hint`). This is for sandbox wrappers; Herdr only inspects the host-visible process.
- The probe is rate-limited (`should_probe_foreground_job`, `pane.rs`). When the shell returns to its prompt, the agent is cleared (the `pending_foreground_shell_clear` and "process_exited" paths).
- After a new agent is acquired there is a 3 s startup grace window (`AGENT_STARTUP_GRACE_WINDOW`, `src/pane/agent_detection.rs:12`). During it the state stays `unknown`.

**Step B: state from the screen** (screen-scraping against a regex/rule manifest)
- Input: `terminal.detection_text()` (`src/pane/terminal.rs:2191,2831`). It returns the last N rows of the live libghostty screen, where N is the pane's row count. It is not the user-scrolled viewport.
- Also input: the latest OSC 0/2 terminal title and OSC 9 progress (`src/pane/osc.rs`, `agent_osc_title()`).
- Engine: `src/detect/manifest.rs` (`detect_with_osc`, region selectors such as `bottom_non_empty_lines(N)`, `after_last_horizontal_rule`, `prompt_box_body`, `osc_title`, `whole_recent`; `contains`, `regex`, `line_regex`, `all`, `any`, `not`, and a priority per rule).
- Claude manifest: `src/detect/manifests/claude.toml` (version 2026.09.11.1, alias `claude-code`). The highest-priority matching rule wins.
  - **working**:
    - OSC title starts with a braille spinner or half-circle, `^[\x{2800}-\x{28FF}\x{25D0}-\x{25D3}] ` (priority 1100).
    - Bottom 12 lines contain `esc to interrupt`, or a `<glyph> Verb… (12s` activity line (970).
    - `Waiting for N background agents` above the prompt box, or `N MCP tasks still running` (965).
    - `/btw` overlay (975).
  - **blocked**:
    - `esc to cancel` below the last horizontal rule, together with `enter to confirm` or `enter to select` plus arrow-navigation hints (980).
    - "Run a dynamic workflow?" (980).
    - MCP elicitation `requests your input` with Accept/Decline (980).
    - Bash permission: `Do you want to proceed?` plus `1. Yes` / `2. No` options (850).
    - Generic permission prompt (840).
    - A legacy catch-all (300).
  - **idle**: the prompt box body begins with `❯` and there is no selection menu (950); OSC title starting with `✳ ` (250); OSC progress `4;0` (250).
  - **unknown/skip**: `showing detailed transcript` viewer and the model picker. These set `skip_state_update` so the state is not changed.
  - If no rule matches for a known agent other than Codex, the fallback is `idle` (`default_known_agent_idle_fallback`). Codex falls back to `unknown`.
- Debounce: a working-to-idle flip is held until 3 consecutive rechecks or 700 ms (`PendingIdleConfirmation`, `agent_detection.rs:6-70`). Idle rescans are skipped while the detection content sequence is unchanged (`should_skip_idle_screen_scan`).
- Screen versus hook state: `set_detected_state_with_screen_signals_at` (`src/terminal/state.rs:~358`) ignores `visible_idle` and `visible_working`, so screen "working" is just the manifest result.
  - `full_lifecycle_hook_authority` (`detect/mod.rs:323`) lists the agents whose integration reports full state and so overrides the screen: pi, omp, mastracode, opencode, kilo, kimi. Claude is not in it. Claude and Codex are in `agent_resume.rs:331`'s list, which covers session identity only.
  - A stale code comment in `detect/mod.rs` says "PTY activity is the normal working authority". I found no PTY-activity working authority in the source. The blog post says activity heuristics were rejected ("Terminal activity is evidence. Lifecycle is stronger."). Treat that comment as stale (unverified beyond a grep for `pty_activity` and similar).
- Manifests are hot-updatable. Bundled ones are in the binary; herdr.dev ships remote updates (`detect/manifest_update.rs`). Local overrides live at `~/.config/herdr/agent-detection/<agent>.toml` and always win. Apply with `herdr server reload-agent-manifests` or `update-agent-manifests`. New agents still need a binary update.
- Live proof (measured): `herdr agent explain $HERDR_PANE_ID --json` on this Claude pane matched rule `osc_title_working` on the title `◐ <session title>`. The other panes report `agent:"claude"`, `agent_status:"idle"`, `terminal_title:"✳ <title>"`.
  - So the Claude Code OSC title spinner (✳ idle, braille/half-circle working) is the primary working/idle signal today, ahead of screen text.
- Reported (not detected) state: any process can call `pane.report_agent` with `--state working|idle|blocked --seq N --source <id>` and optional `-- resume argv` (`add-herdr-support.mdx`). Newer `seq` wins. Herdr clears the agent a second or two after the pane returns to a shell prompt.
  - Presentation-only metadata goes through `pane.report_metadata` (`--title`, `--display-agent`, `--state-label`, `--token`). Tokens are shown in the sidebar and never change semantic state.

## 5. Herdr plugin system
Sources: `docs/.../plugins.mdx`, `marketplace.mdx`, `socket-api.mdx:447-605`, https://herdr.dev/docs/plugins/, https://herdr.dev/docs/marketplace/, https://herdr.dev/plugins/.
- **What a plugin is**: a directory with `herdr-plugin.toml` plus commands. Herdr only launches argv arrays (no shell); the language is up to the plugin (Bash, Node/Bun, Lua, Rust, Go, Python, PowerShell).
  - There is no SDK. "The entire Herdr CLI is the plugin API" (`plugins.mdx`). Plugins call `$HERDR_BIN_PATH` or the socket.
  - Runtime action registration and native non-terminal plugin UI are explicitly not part of v1.
- **Manifest** (required: `id`, `name`, `version`, `min_herdr_version`):
  - Optional top-level: `description`, `platforms[linux|macos|windows]`.
  - `[[build]]` runs only on `herdr plugin install` from GitHub, not on `link`.
  - `[[startup]]` runs once after restore, when the API is ready.
  - `[[actions]]` has `id`, `title`, `contexts` (`global`, `workspace`, `pane`), and `command`; it is invokable by name or key.
  - `[[events]]` has `on = "<event>"`, for example `worktree.created`.
  - `[[panes]]` has `id`, `title`, `placement = overlay | popup | split | tab | zoomed`, and `command`. Popups accept `width`/`height` (`"80%"` or cells).
  - `[[link_handlers]]` has `pattern` (a Rust regex) and `action`, and fires on Ctrl-click of a matching URL.
- **Runtime env** injected into plugin commands: `HERDR_SOCKET_PATH`, `HERDR_BIN_PATH`, `HERDR_ENV`, `HERDR_PLUGIN_ID/ROOT/CONFIG_DIR/STATE_DIR`, `HERDR_PLUGIN_CONTEXT_JSON` (workspace, tab, pane, worktree, agent, selected text, clicked URL), `HERDR_WORKSPACE_ID/TAB_ID/PANE_ID`, plus `HERDR_PLUGIN_ACTION_ID`, `HERDR_PLUGIN_EVENT(_JSON)`, `HERDR_PLUGIN_ENTRYPOINT_ID`.
  - There is no managed storage API; plugins own their files under the config and state dirs.
- **Install and load**:
  - `herdr plugin install owner/repo[/subdir] [--yes] [--ref]` clones with git, shows a preview, runs build commands, and registers it. `herdr plugin link <dir>` is for local development.
  - Other subcommands: `uninstall`, `unlink`, `enable`, `disable`, `list`, `config-dir`, `action list|invoke`, `log list`, `pane open|focus|close`.
  - The registry is `plugins.json` beside `session.json`; plugins are global to the user across sessions.
  - There is no `plugin update`; reinstall to refresh. Plugins are not sandboxed or reviewed; they run as the user.
  - Keybinding: `[[keys.command]] type = "plugin_action" command = "<plugin.id>.<action>"`.
  - Live check on this machine: `herdr plugin --help` works; `~/.config/herdr/plugins` does not exist and `.plugins.lock` is empty (0 B), so no plugins are installed here.
- **Marketplace** (https://herdr.dev/plugins/): an automatic index of public GitHub repos with topic `herdr-plugin` and a parseable `herdr-plugin.toml` at the root or in a subdirectory. It refreshes every 30 minutes, with no review.
  - The index file (https://assets.herdr.dev/plugins/index.json, generated 2026-10-08T10:30Z) says `pluginCount` 1614 across `repositoryCount` 1560. The homepage counter said 1,548, so it lags the feed.
  - Repo languages: Rust 349, Shell 322, Python 314, JS 216, Go 170, TS 143, PowerShell 11, Lua 9.
  - Platform declarations: about 1,180 linux+macos, 212 all three OSes, 99 macOS-only.
- **Representative plugins** (stars from the index; manifests fetched from the repos' default commits):
  - `zenbu-labs/terminal-browser` (3,703 stars): "A browser inside your terminal". One `[[actions]]` entry splits the focused pane and opens it. The `[[build]]` step runs `curl -fsSL https://terminal-browser.sh/install | bash`, an example of install-time arbitrary code.
  - `zenbu-labs/terminal-code` (2,132): "VS Code in the terminal".
  - `persiyanov/herdr-reviewr` (850): a pane plugin with `placement="split"` plus toggle/open/close actions and `[[events]]` on `worktree.created/opened`. It reviews the agent's diff and sends line comments back into the agent's input (`pane send-text`). Per its comment it relies on `session.snapshot` and live event subscriptions added in 0.9.3.
  - `smarzban/herdr-file-viewer` (635) and `alexarthurs/herdr-sidebar` (452): VS Code-style file explorer and git panes opened as split panes. Their manifests warn that Windows cannot spawn relative `[[panes]]` commands, so they use per-OS actions.
  - `eliasstravik/herdr-projects` (587): `[[startup]]` plus many actions, a popup, and metadata tokens, giving a "coordinator conversation, parallel worker threads, shared memory" layer on top of Herdr.
  - External clients built as plugins or companions: `AltanS/collie` (1,256 stars, a Bun/PWA mobile web UI served over Tailscale), `devswha/herdr-web-ui` (644, browser and phone chat plus live terminal; its `[[startup]]` starts a bridge), `ZingerLittleBee/Heeler` (510, a native iOS app over SSH using libghostty), `dcolinmorgan/herdr-remote` (407, menu bar, phone, Telegram), `furkankly/zoetrope` (1,013, a flow graph of a Claude/Codex session). These show that a GUI over the socket API is an established pattern.
- **Examples cookbook**: `ogulcancelik/herdr-plugin-examples` (agent-telegram-notify, github-link-preview, dev-layout-bootstrap), cited in `plugins.mdx`.
- **What plugins can and cannot contribute**: actions, event hooks, startup hooks, terminal panes (including overlay/popup), URL click handlers, keybindings, sidebar display tokens (`pane.report_metadata`, `workspace.report_metadata`), an agent-list projection (`agent.view.set`), agent state and session reports, and layout application. They cannot add native non-terminal UI, runtime-registered actions, or custom detection manifests (manifests are an agent/config feature, not plugins).

## 6. Herdr website claims (compare, cloud, blog)
- **Homepage** https://herdr.dev/: "the agent runtime. Run them anywhere. Leave them running."
  - Stats: 42,441 GitHub stars, 1,293,735 installs, 1,548 community plugins, 22 agent CLIs detected. The stats feed says 42,890 stars, generated 2026-10-08T10:49Z.
  - Banner: "We raised $6M." Installers: curl for macOS/Linux, PowerShell for Windows, brew, mise, nix.
  - Claims: always running, never hunt for the stuck one, agent-native CLI/socket API, "Runs what you already run", all your machines in one Herdr.
- **Compare** https://herdr.dev/compare/: "Apps manage the herd. Herdr runs it." Positions itself as a runtime with clients, not an app. It compares against tmux/zellij, cmux/warp, solo, and conductor/emdash/superset.
  - Its capability matrix rows: work survives UI closing (yes, the server owns terminals), runs inside your existing terminal (yes, unlike cmux, warp and the manager apps), semantic agent state (blocked/working/done/idle), detach and SSH, direct attach to one agent, agent-driven API (read/send/wait/split/attach), worktree and diff review (it "pairs with" tools; manager apps have it as their core), and multiple clients on the same runtime ("TUI · CLI · plain SSH, more coming").
  - Differentiators claimed: persists agents rather than only terminals, knows blocked state, and waits on agents without polling.
- **Cloud** https://herdr.dev/cloud/: "Herdr Cloud, coming soon". It would connect your own machines to one Herdr client without SSH setup (waitlist only). The blog says the traffic would be end-to-end encrypted, Cloud only brokers connections, and it does not host agents.
- **Blog** https://herdr.dev/blog/:
  - 2026-09-08 "Herdr raised a $6M seed" (Bessemer-led, with YC and others). 2026-08-06 "joining Y Combinator" (the runtime stays Apache-2.0).
  - 2026-09-07 "Connecting the machines". 0.9 moves the outer UI into the client so one TUI can federate independent servers. Cross-machine agent CLI is a stated next step, and moving a session between machines is a stated far-out idea.
  - 2026-08-03 "Ten agents, three clients, 95% less CPU". The server no longer animates the sidebar spinner, skips frames for hidden panes, and no longer repaints on mouse motion. Terminal parsing still runs for hidden panes.
  - 2026-06-10 "Coding agents are becoming runtimes". It argues for a standard lifecycle contract (working/blocked/idle plus session id). It says screen reading broke when agent UIs changed, so Herdr now uses hooks where agents expose them and ships hot-reloadable manifests.
  - 2026-05-27 "Live updates without killing your terminal processes" (live handoff via PTY master fd transfer) and "Herdr 0.6.3".
- The marketing claims above are the site's own and were not independently verified. The stats are reported as published.

## 7. Gaps and things a desktop GUI (Wings) could do better
- **Session identity for Claude**: Herdr depends on a `SessionStart` hook in `~/.claude/settings.json` and reports `claude --resume <id>` as typed shell input. A GUI can verify the hook (its own status check does not) or skip hooks and read `~/.claude/projects/*/*.jsonl` and `~/.claude/sessions`. It could also track `/clear`, `/compact` and `/fork` rollover, since the `source` values arrive as separate hook events.
- **Resume fidelity**: Herdr resumes Claude with a bare `claude --resume <id>`, so flags such as model and permission mode are lost unless the agent reports its own `resume_argv`. The GUI can persist the full launch argv per tab.
- **State quality for Claude**: it is fully screen-scraped. The 9 KB manifest has priority-ordered regexes tied to Claude's UI text, with a documented "unusual new prompts may show as idle" failure mode (`agents.mdx`, "Blocked state").
  - The GUI could combine Claude Code hooks (`PermissionRequest`, `Notification`, `Stop`, `UserPromptSubmit`, `SessionEnd`) with the OSC title as a fallback. This is exactly what Herdr v≤9 did and then removed (`claude_settings.rs:26-56`). I did not verify why they removed it; it may have been hook-timing gaps around interrupts and cancels, and they cite a similar reason for Devin and Droid in `integrations.mdx`.
- **Native chrome the TUI cannot do**: real rich panes (diff/review/transcript views, markdown, images), drag-drop between spaces, context menus, system notifications that focus the right pane (Herdr's macOS `system` mode falls back to `osascript`, which cannot activate the terminal), per-agent usage/cost, and a true overview of all agents without Unicode width constraints.
- **Plugins**: Herdr plugins are process-based, TUI-in-a-pane only, with no UI API, no managed storage, no update command, no sandbox, and curl-pipe-bash build steps. Wings could offer a declarative UI-contribution API (webview panels), permissioned capabilities, signed or pinned installs, and a registry with updates. It could still import Herdr's `herdr-plugin.toml` actions and events as a compatibility layer, since the CLI and socket surface is stable and documented.
- **Scrollback and history**: Herdr's `pane_history` stores raw ANSI in a single JSON file, replayed only when the layout fingerprint matches exactly, and it is plaintext on disk (docs warn about secrets). A GUI could store transcripts per session in a proper local database with search and redaction.
- **Process survival**: Herdr's own limitation is that process survival across a machine or server restart is not possible. Only layout, cwd and resumable agents come back. Wings has the same constraint unless it keeps its own daemon.
- **Multi-machine**: Herdr needs SSH (Cloud is a waitlist). `herdr --machine` CLI forwarding exists, but IDs and agent names are scoped per server, and the agent CLI cannot see other machines' agents.
- **Detection beyond screens**: Herdr cannot see through tmux inside a pane or a sandbox wrapper without `HERDR_AGENT` (`agents.mdx`). A GUI that owns the PTY and the launch command knows the agent kind without inspecting the process tree.

## 8. Unverified or open
- Whether a 0.9.3 server accepts `resume_argv` (docs say 0.10+; code is present in the tag).
- Why the herdr hook entry is missing from this machine's `~/.claude/settings.json`.
- Claims on herdr.dev (star and install counts, "22 agents", CPU savings) are as published and were not independently tested.
- I did not run any state-changing herdr command; behavior of install, update, and live handoff is from the docs and source only.
- Windows-specific code paths were not read.
- `docs/next/` may be ahead of 0.9.3; the llms.txt index links to it for v0.9.3, but the 0.10.0 mention shows the docs are slightly ahead of the binary.