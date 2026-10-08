# Wings plan

Wings is a cross-platform desktop app (macOS, Windows, Linux) for running Claude Code sessions, in the spirit of Herdr. It has real terminals, a space per project, live session state, per-project history with search and resume, and plugins that add UI and agent tools. Claude Code is the only agent supported in v1.

Status: plan only, no code yet. Written 2026-10-08. The evidence behind each decision is in `docs/research/`:

- `docs/research/herdr.md`: Herdr features and internals (v0.9.3 source)
- `docs/research/claude-code-integration.md`: hooks, transcripts, resume, MCP and plugins in Claude Code 2.1.294
- `docs/research/desktop-and-terminal-stack.md`: Tauri, GPUI, xterm.js, Ghostty, PTY, packaging
- `docs/research/plugins-and-mcp.md`: MCP 2026-07-28, rmcp, plugin runtimes, sandboxing
- `docs/research/landscape.md`: about 25 existing session managers and what they do

---

## 1. What v1 does

| Area | v1 behaviour |
|---|---|
| Spaces | Sidebar lists projects (one per folder), with git branch and a rolled-up status dot, as in Herdr. Worktrees group under their parent repo. |
| Terminals | Tabs and splits of real shells. Fast enough for 12 panes of Claude Code redrawing at once. |
| Session discovery | You type `claude` in any Wings pane. Wings binds that pane to the Claude session within one hook call, with no flags and no wrapper. |
| Live state | Each session shows working, blocked (permission or question), done-unseen, idle or ended, plus current tool, context use, cost and model. State rolls up from pane to tab to space. |
| History | A per-project list of past sessions with full-text search and filters, plus resume and fork in a new tab. |
| Live views | Panels driven by the session as it runs: activity timeline, todos, files changed, subagents, cost and context. |
| Single MCP | One Wings MCP server registers itself with Claude Code. It exposes Wings tools plus every plugin's tools, and the tool list updates live when plugins change. |
| Plugins | Third parties ship views (UI), agent tools, commands and event handlers in one package. Permissions are deny-by-default. |
| Notifications | System notification when a session blocks or finishes. Clicking it focuses the pane. |

Not in v1: agents other than Claude Code, remote machines or SSH, keeping processes alive across an app restart (we resume instead, see 4.6), a public plugin registry, and plugin signing.

---

## 2. Decisions

Each decision names the nearest rival and what rules it out.

| # | Decision | Rival | Why the rival loses |
|---|---|---|---|
| D1 | **Tauri 2.12.x**, stable wry runtime (WKWebView, WebView2, WebKitGTK) | GPUI with `alacritty_terminal` | GPUI is the fastest at drawing terminals, but it can't host HTML/JS plugin UI: `gpui-webview` is experimental with no Wayland support. Also, the crates.io `gpui` is 0.2.2 from 2025-10 and `gpui_platform` is unpublished. Tauri 3 with CEF is alpha only. |
| D2 | **xterm.js 6 in the webview**, with Rust owning the PTYs. It sits behind a swappable `TerminalView` interface. | Rust VT (`alacritty_terminal` or `libghostty-vt`) plus our own canvas renderer | The rival is the planned fallback, not the start. With it we'd own selection, IME, fonts, links and a11y. TUICommander had to patch `alacritty_terminal` for Claude Code's Ink redraws. The spike S1 gate (section 6) decides. |
| D3 | **Wings installs a Claude Code plugin (user scope)** that bundles the hooks and the MCP server. Both no-op outside Wings. | Pass `--settings` and `--mcp-config` on each launch (what cc-pane does) | You start `claude` yourself, so Wings never sees the command line. A PATH shim breaks with aliases and absolute paths. Also, those flags are not restored on `--resume`. |
| D3b | (same) | Edit `~/.claude/settings.json` directly (what Herdr does) | On this machine Herdr's hook script exists, but its `settings.json` entry is gone, and `herdr integration status` still says "current". A plugin is installed, versioned and removed as one unit. |
| D4 | **Hooks are the main state signal.** The OSC terminal title and process inspection are cross-checks. | Screen scraping with regex manifests (what Herdr does now) | Hooks are documented and structured, and every event carries `session_id`, `transcript_path` and `cwd`. Screen rules break when Claude's UI text changes. We keep the OSC title spinner check because Herdr shows it is the most reliable screen signal (measured live: `✳` is idle, braille/half-circle is working). |
| D5 | **Transcript JSONL is read behind a versioned adapter.** It feeds history and live content, never state. | Use transcripts for everything | The docs call the entry format internal and say it "changes between versions". Writes lag turns (observed). |
| D6 | **SQLite with FTS5** for the session index, layouts and plugin storage | Scan on demand | 1,457 transcripts on this machine already. Search and filters need an index. |
| D7 | **`wings-mcp` is a stdio bridge** that Claude Code spawns. It relays to the app over a local socket. | Claude connects to an HTTP endpoint in the app | With a bridge, sessions outside Wings get a clean zero-tool server instead of "Failed to connect". The bridge survives app restarts and reconnects on its own (Claude Code doesn't reconnect stdio servers itself). It inherits the pane's env, and the stdio idle timeout is 30 min against 5 for HTTP. |
| D8 | **MCP server built on rmcp 3.5.x**, handling both MCP eras (2026-07-28 stateless and 2025-11-25 sessions) | Hand-rolled JSON-RPC | Claude Code picks its v1 or v2 MCP runtime per launch, so we must serve both, and rmcp 3.5.1 does. |
| D9 | **Plugins are web packages.** UI runs in sandboxed iframes and logic in a Web Worker. The only channel to the host is postMessage JSON-RPC, using the MCP Apps dialect plus `wings/*` methods. | WASM components on Wasmtime (what the plugin research recommends) | Plugin UI has to be web anyway, so WASM logic gives TS authors a second toolchain: Javy (≥869 KB per module) or the experimental ComponentizeJS. WASI 0.3 toolchains are "still landing", and Extism pins an unsupported Wasmtime. A WIT tier stays possible later. Spike S3 must pass first (sandbox escape and throttling). |
| D10 | **"Process plugins" as an explicit high-trust tier**: any stdio MCP server, aggregated behind the one Wings endpoint | No native-code plugins | This covers other languages and native tools now. It runs as the user with no sandbox, so it gets its own install consent. |
| D11 | **No daemon in v1.** PTYs live in the app process. On restart, Wings restores the layout and resumes each Claude session with its original flags. | A `wingsd` daemon that owns PTYs (Herdr, Superset) | A daemon doubles the IPC surface before the UI is proven. Resume is cheap: `claude --resume <id>`. `CLAUDE_CODE_RESUME_INTERRUPTED_TURN=1` may continue a turn that was cut off mid-way, but the docs describe it for SDK mode, so S2 checks whether it works in the TUI. `wings-core` is a library from day one, so a daemon can host it later. |
| D12 | **React 19 + TypeScript + Vite** for the app UI | SolidJS (TUICommander) | The terminals are canvas/WebGL either way, so the framework barely affects speed. React has the MCP Apps React hooks and a bigger ecosystem. Plugin UIs are framework-free, since they're iframes. |

---

## 3. Architecture

```
 ┌──────────────────────────── Wings app (Tauri 2) ─────────────────────────────┐
 │  Webview (React)                                                             │
 │   sidebar · tabs/splits · xterm.js panes · history · live views              │
 │   plugin iframes (sandboxed, custom scheme)  plugin workers                  │
 │        ▲ Channel (raw bytes)     ▲ invoke/events        ▲ postMessage        │
 │  ──────┼─────────────────────────┼──────────────────────┼──────────────────  │
 │  Rust core                                                                   │
 │   wings-pty ── PTYs, process tree, OSC tap                                   │
 │   wings-claude ── hook ingest · transcript tailer · resume argv · CC plugin  │
 │   wings-index ── SQLite + FTS5 session index                                 │
 │   wings-core ── spaces/tabs/panes/sessions model · event bus · persistence   │
 │   wings-plugins ── manifests · permissions · asset scheme · RPC broker       │
 │   wings-mcp ── rmcp server: built-in tools + plugin tools                    │
 │   local socket (UDS / named pipe) ◄──────────────┐                           │
 └──────────────────────────────────────────────────┼───────────────────────────┘
                                                    │
   PTY child: your shell → `claude`                 │
   env: WINGS_ENV=1 WINGS_PANE_ID WINGS_SOCKET WINGS_PANE_TOKEN
     │                                              │
     ├─ hooks (from the Wings CC plugin) ── wings-hook ──► socket
     └─ MCP stdio (from the Wings CC plugin) ── wings-mcp bridge ──► socket
```

### 3.1 Repository layout

```
wings/
  Cargo.toml                    # workspace
  crates/
    wings-core/                 # domain model, event bus, persistence API
    wings-pty/                  # portable-pty, foreground-job inspection, OSC/progress tap
    wings-claude/               # Claude Code adapter (hooks, transcripts, resume, CC plugin installer)
    wings-index/                # SQLite + FTS5 index, incremental ingest
    wings-mcp/                  # rmcp server, tool registry, list_changed fan-out
    wings-plugins/              # manifest, permissions, asset protocol, RPC broker
    wings-protocol/             # shared types → JSON Schema + TS (ts-rs or specta)
  bins/
    wings-hook/                 # tiny: read hook JSON on stdin → socket; exit 0 fast
    wings-mcp-bridge/           # stdio MCP server that relays to the app
  apps/desktop/                 # Tauri app: src-tauri/ + web/ (React)
  integrations/claude-code/     # template of the Wings Claude Code plugin
  packages/
    plugin-sdk/                 # TS SDK for plugin authors (types generated from wings-protocol)
    create-wings-plugin/        # scaffolder
  examples/plugins/             # reference plugins (also our dogfood)
  docs/
```

---

## 4. Components

### 4.1 Terminal (`wings-pty` + `TerminalView`)

- **PTY.** `portable-pty` 0.9.0 (ConPTY on Windows 10 1809+). On Windows, sideload a current `conpty.dll` + `OpenConsole.exe` from Microsoft's ConPTY nupkg, because an old bundled pair crashed pwsh in wezterm. The xterm side must answer ConPTY's startup `ESC[6n`, or reads stall.
- **Shell detection.** Unix: `$SHELL`, then passwd, then `/bin/sh`. Windows needs our own list: pwsh, Windows PowerShell, cmd, Git Bash, WSL. portable-pty only checks `%ComSpec%`.
- **Pane env.** `WINGS_ENV=1`, `WINGS_PANE_ID`, `WINGS_SOCKET`, `WINGS_PANE_TOKEN` (per-pane secret), plus `TERM_PROGRAM=wings`.
- **Streaming.** Coalesce PTY reads per frame, to at least about 1 KB. Send them as `InvokeResponseBody::Raw` over a Tauri Channel. `Channel<Vec<u8>>` is not raw: it arrives as a JSON array of numbers. Apply xterm.js watermark flow control (HIGH ≤ 500 KB).
- **Rendering.**
  - WebGL addon only for visible panes. WebKit caps a page at 16 WebGL contexts and silently drops the oldest one.
  - On `onContextLoss`, fall back to the DOM renderer.
  - Linux defaults to DOM until S1 proves WebGL fast there. Tauri warns that WebKitGTK reports WebGL as working even when it is software-rendered.
  - Hidden tabs keep their xterm instance, which keeps parsing, but drop their renderer.
- **Windows xterm option.** `windowsPty: { backend: 'conpty', buildNumber }`.
- **Rust-side tap.** `wings-pty` reads OSC 0/2 (title), OSC 9;4 (progress) and alt-screen enter and exit with a light `vte` parser. This is not a full screen model. It feeds D4's cross-check.
- **Claude Code's fullscreen renderer** (alt screen, mouse capture, DEC 2026 synchronized output) is the default for new sessions since 2026-05-06, so test with it.
- **Known open bug to watch.** xterm.js #5847 (row ghosting during Claude Code streaming, seen in Tauri WKWebView and Electron).
- **Linux NVIDIA.** Set `__NV_DISABLE_EXPLICIT_SYNC=1`. Set `WEBKIT_DISABLE_DMABUF_RENDERER` only when needed (Tauri's documented order).

### 4.2 Claude Code integration (`wings-claude`)

**The Wings Claude Code plugin** is generated into Wings' data folder and installed with `claude plugin marketplace add <dir>` then `claude plugin install wings@wings --scope user`. Wings asks first, shows status in settings, and offers uninstall. It contains:

- `hooks/hooks.json`: command hooks in exec form (`args` present, `command` = absolute path to `wings-hook`, which on Windows must be a real `.exe`). Non-blocking events are `async: true`. Events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `PermissionDenied`, `Notification`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`, `PreCompact`, `PostCompact`, `CwdChanged`, `SessionEnd`.
- `.mcp.json`: one stdio server `app` → `wings-mcp-bridge`. Tools appear as `mcp__plugin_wings_app__<tool>`.
- A skill that tells Claude what the Wings tools are for.

**`wings-hook`.** It exits 0 at once if `WINGS_ENV` is unset, so it costs nothing outside Wings. Inside Wings it forwards `{pane_id, pane_token, hook_json}` to `WINGS_SOCKET` and exits. It has no stdout unless Wings returns a decision. That only happens later, for answering permission prompts from the UI.

GUI apps on macOS and Linux don't inherit the shell `PATH`, so use `fix-path-env-rs` before running `claude plugin ...`. On this Mac `claude` is in `~/.local/bin`.

**Session binding.** `SessionStart` (source `startup|resume|clear|compact|fork`) binds pane → `session_id` + `transcript_path`. On `/clear` the session id changes, and the same pane re-binds.

**State machine (per session):**

| Signal | State |
|---|---|
| `SessionStart` | idle |
| `UserPromptSubmit` | working |
| `PreToolUse` (tool) | working, current tool = tool |
| `PreToolUse` with `AskUserQuestion` | blocked: question |
| `PermissionRequest` | blocked: permission (fires right away; the `Notification` permission_prompt fires about 6 s late) |
| `Notification` `elicitation_dialog` | blocked: MCP input |
| `PostToolUse` / `PostToolUseFailure` / `PermissionDenied` | working |
| `Stop` | done (unseen) → idle once you view the pane |
| `StopFailure` | error |
| `SessionEnd`, or the process leaves the foreground job | ended |
| OSC title `✳ …` held for 700 ms while state says working | idle (catches Esc interrupts, which may not fire `Stop`; to confirm in S2) |

**Fallback when the plugin is missing or disabled.**

1. Process inspection finds `claude` as the pane's foreground job, the way Herdr does (libproc + `KERN_PROCARGS2` on macOS, `/proc` on Linux, Toolhelp on Windows).
2. Map PID → session with `~/.claude/sessions/<pid>.json`. This file is undocumented but works here: it has `sessionId` and `status` busy/idle. Use the documented `claude agents --json` to reconcile.
3. State comes from the OSC title only.
4. The pane shows a "limited tracking" badge.

**Resume fidelity.** Read the live `claude` argv from the process table. On restore or resume, run `claude --resume <id>` plus the original flags, minus `--resume`, `--continue` and `--session-id`. Herdr types a bare `claude --resume <id>`, which loses model and permission flags. Fork is `--resume <id> --fork-session`.

**Metrics without the statusline.** Plugins can't set `statusLine`, and we must not replace yours (claude-hud). So we take model, usage and cost from the transcript: assistant `message.usage` for tokens and context %, `cost-state` entries for cost.

### 4.3 Transcripts and history (`wings-index`)

- **Source.** `~/.claude/projects/<cwd with non-alphanumerics → '-'>/<session-id>.jsonl` (matched 1,457/1,457 here), plus `<session-id>/subagents/agent-*.jsonl`. Respect `CLAUDE_CONFIG_DIR`.
- **Adapter.** A tolerant line parser. Unknown `type` values are counted and skipped, never fatal. Types we use: `user`, `assistant`, `system` (`compact_boundary`, `away_summary`), `ai-title`, `custom-title`, `cost-state`, `pr-link`, `worktree-state`, `agent-name`, `last-prompt`. Each Claude Code version we see is recorded, so a format change shows up as a spike in unknown types.
- **Ingest.**
  1. On first run, do a full scan. S4 measures it.
  2. After that, a `notify` file watcher plus a byte offset and mtime per file. Parse only appended bytes, up to the last newline.
  3. Live sessions are tailed directly from `transcript_path`.
- **Per session we store:**
  - title (custom-title, else ai-title, else first prompt), cwd → space, git branch, worktree
  - first and last timestamp, model(s), turn count, tokens, cost
  - tools used, files touched (from Edit/Write/NotebookEdit inputs), subagent count, PR links
  - whether the transcript still exists
- **FTS5** over prompts, assistant text, titles and file paths.
- **Retention.** Claude Code deletes transcripts older than `cleanupPeriodDays`, default 30. Wings keeps the indexed metadata and text, and marks those sessions "not resumable". Settings shows the current value with a hint to raise it.
- **History view.**
  - It lives under each space.
  - Filters: text search, date range, branch, model, has-PR, cost, tool, file touched.
  - Actions: resume, fork, open transcript, copy resume command.
  - A global view covers all spaces.

### 4.4 Live session views

These are built-in, and from M6 they move onto the plugin API to prove it:

- **Activity timeline**: prompts, tool calls with duration, permission waits, compactions.
- **Tasks**: from `TaskCreated`/`TaskCompleted` hooks and `TaskCreate`/`TaskUpdate` tool inputs. These only exist in sessions that have the Task tools. None of the last 150 local transcripts used them, so this view is secondary.
- **Files changed** in this session, with a click to open a diff.
- **Subagents tree**, from `SubagentStart`/`Stop` and the subagent transcripts.
- **Context and cost meter**, plus a PR chip from `pr-link`.
- **Sidebar card** per agent: state, current tool, elapsed time, context %.
- **Live assistant text** is opt-in. The `MessageDisplay` hook streams lines, but Claude holds each batch until the hook returns, so S2 must show it adds no visible lag first.

### 4.5 The single MCP server (`wings-mcp` + `wings-mcp-bridge`)

- **Bridge.**
  - Claude Code spawns `wings-mcp-bridge` over stdio, and it inherits the pane env. stdio servers inherit the shell environment unless `CLAUDE_CODE_MCP_ALLOWLIST_ENV=1`.
  - The bridge is a real rmcp server. It connects to the app socket with `WINGS_PANE_TOKEN` when present. If that is missing, it uses the app's well-known socket as an unbound session.
  - If the app is down, it serves zero tools, retries in the background, and sends `tools/list_changed` when the app comes back.
- **Tool list is per caller.** The token maps to pane, then session, then space, then the plugins enabled for that space. A change fans out `list_changed` to the affected bridges (legacy peer notify and the 2026-07-28 `subscriptions/listen` sink).
- **Names.**
  - Built-in tools are `<verb>_<noun>`. Plugin tools are `<plugin-id>_<tool>`. Plugin ids are `[a-z0-9-]+`, so the first `_` splits them.
  - No dots: the Claude API tool-name regex is `^[a-zA-Z0-9_-]{1,128}$`.
  - Claude Code truncates descriptions at 2,048 chars and loads definitions on demand through tool search, so keep descriptions short.
- **Built-in tools in v1:** `list_sessions` (live sessions and their states), `search_history` (past sessions in this project), `read_session` (summary and messages of one), `notify` (desktop notification), `open_pane` (run a command in a new pane, for example a dev server), `read_pane` (recent output of a pane).
- **Limits.** Results over 10k tokens warn and the default cap is 25k, so paginate. Long tools send progress notifications. A call over 2 minutes becomes a background task in Claude Code.

### 4.6 Persistence and restore (`wings-core`)

- **Layout snapshot.** Spaces, tabs, a BSP split tree, cwd, focus, the bound session id and launch argv per pane. It's stored in SQLite and written on change, debounced.
- **On start:**
  1. Restore the layout.
  2. Start each pane's shell.
  3. For panes that had a Claude session, run the resume command.
  4. Space them about 100 ms apart, as Herdr does.
- **Scrollback.** Not stored in v1. Herdr's raw-ANSI history file is plaintext, and its docs warn it can hold secrets.

### 4.7 Plugin system (`wings-plugins`)

**Package:** a folder with `wings-plugin.json` (JSON, not TOML, so plugin authors need no extra format), web assets, and JSON schemas for tools. API 1 is built: see `plugins/README.md`. The block below is the target shape, which API 1 only partly covers.

```toml
[plugin]
id = "git-insights"            # [a-z0-9-]+
name = "Git Insights"
version = "1.2.0"
api = "1"                      # Wings plugin API major
min_wings = "0.4.0"

[permissions]                  # deny by default, shown at install
events = ["session.state", "session.tool", "session.message"]
transcript = "summary"         # none | summary | full
net = ["api.github.com"]
storage = "kv"
commands = ["pane.open"]       # host actions the plugin may call

[worker]                       # optional background logic
entry = "dist/worker.js"

[[views]]
id = "panel"
entry = "dist/panel.html"
slot = "session-side"          # sidebar | session-side | bottom | tab | status-item

[[tools]]
name = "blame_range"           # exposed as git-insights_blame_range
description = "Show who last changed a line range"
input_schema = "schemas/blame.json"
confirm = "first-use"          # never | first-use | always (host enforces)

[[commands]]
id = "open-report"
title = "Git Insights: open report"
keybinding = "cmd+shift+g"

[[panes]]                      # TUI-style plugins: run a command in a terminal pane
id = "lazygit"
command = ["lazygit"]
placement = "split"
```

**Runtime:**

- **Asset scheme.** Plugin assets are served from one custom scheme, with the plugin id in the path. That is `wings-plugin://localhost/<id>/<run>/` on macOS and Linux, and `http://wings-plugin.localhost/<id>/<run>/` on Windows, where `run` is new on each start so WebKit's cache never serves an old file, because WebView2 serves custom schemes under http. The scheme handler sets each plugin's CSP header.
- **Views** are `<iframe sandbox="allow-scripts">` with no `allow-same-origin`. That gives every plugin frame an opaque origin, so sharing a scheme doesn't let plugins reach each other. The broker identifies the sender by its frame (`event.source`), never by a claimed id. Each view gets a strict CSP that starts from the MCP Apps default (`connect-src 'none'`) and opens only the hosts in `net`.
- **Worker.** It runs in a hidden sandboxed iframe as a Web Worker. It receives events and tool calls, and it stays alive while the plugin is enabled.
- **RPC broker.** The host side is the only door. Every postMessage RPC is checked against the manifest permissions in Rust before it's proxied. Plugins never get Tauri `invoke`.
- **SDK** (`@wings/plugin-sdk`): `wings.on(event)`, `wings.tools.handle(name, fn)`, `wings.kv`, `wings.commands.run`, `wings.ui.notify`. Types are generated from `wings-protocol`. Views also speak the MCP Apps `ui/*` methods, so a tool-attached view can render in other MCP Apps hosts.
- **Events.** One normalized, versioned schema (`wings.session.v1`) so a Claude Code format change breaks one adapter, not every plugin:
  - `session.started`, `session.state`, `session.prompt`, `session.tool.started`, `session.tool.finished`, `session.message`, `session.usage`, `session.subagent.*`, `session.todos`, `session.ended`
  - `pane.*`, `space.*`
- **Dev loop.** `wings plugin link <dir>` hot-reloads, with a plugin devtools pane for logs and RPC traffic.
- **Process plugins (D10).** `[mcp] command = [...]` registers a stdio MCP server that Wings starts and aggregates behind the single endpoint. A red-flag consent dialog says it runs as you, unsandboxed.
- **Prompt-injection guard.** Tool descriptions are pinned by hash at install. If a description changes, Wings asks again.

### 4.8 Security boundaries

- **The local socket** is user-only (0600 / pipe ACL). Each pane token is random, per pane, and never written to disk.
- **Tauri.** Capabilities give plugin origins no commands, and `AppManifest::commands` limits what the main webview exposes. On Windows, wry injects the IPC bridge into iframes too. S3 must prove a plugin iframe can't get through the ACL.
- **The Claude Code plugin** only forwards data. It never changes Claude's behaviour unless you act in the Wings UI (later: answering permission prompts).

---

## 5. Milestones

Each milestone ends with a check that runs on macOS, Windows and Linux in CI or by hand.

| M | Scope | Done when |
|---|---|---|
| M0 | Spikes S1 to S4 (section 6). Workspace setup, CI matrix, and the `rustup` toolchain (not installed on this Mac yet). | Every gate has a written result in `docs/spikes/`. D2 and D9 are confirmed or switched. |
| M1 | Terminal shell: spaces, tabs, splits, PTY, themes, keybindings (tmux-style prefix plus native shortcuts), layout persistence. | 12 panes of recorded Claude output meet the S1 targets. Restart restores the layout. |
| M2 | Claude awareness: CC plugin installer, `wings-hook`, state machine, OSC cross-check, agents panel, rollups, notifications, fallback detection. | The S2 scenario script gives the right state for every step. |
| M3 | History: indexer, history view, search and filters, resume and fork, resume-on-restart with original argv. | All local transcripts are indexed. Search returns in < 100 ms. Resume reopens the right session with its flags. |
| M4 | Live views (4.4) and the sidebar agent cards. | Views update within 500 ms of the hook or transcript write. |
| M5 | MCP server: bridge, built-in tools, per-pane tool lists, `list_changed`. | A live Claude session sees a new tool without restarting. A session outside Wings sees a clean zero-tool server. |
| M6 | Plugin API v1: manifest, permissions, views, worker, tools, commands, panes, SDK, scaffolder, dev mode. Port the M4 views onto it. Process-plugin tier. | Two example plugins work end to end on all three OSes. The S3 escape tests stay green. |
| M7 | Shipping: macOS notarization, Windows signing, Linux AppImage/deb/rpm, signed updater, `tauri-action` CI. | A signed build installs and auto-updates on each OS. |

Later: a daemon for process survival, remote machines, answering permission prompts from the Wings UI, more agents (Codex and others), a plugin registry with minisign then Sigstore signing, and a WASM logic tier.

---

## 6. Spikes (M0)

The numbers are our targets, not measured values.

| Spike | Question | Method | Gate |
|---|---|---|---|
| S1 terminal | Is xterm.js fast enough in each webview? | Record PTY bytes from a real fullscreen Claude Code session. Replay into 12 panes (4 visible). Measure frame time, keypress-to-paint latency and CPU on WKWebView, WebView2 and WebKitGTK (X11 and Wayland). Compare xterm WebGL, xterm DOM, and a minimal `alacritty_terminal` + canvas prototype. | Focused pane p95 frame < 16 ms and input p95 < 30 ms on all three. Fail on two or more OSes → switch D2. |
| S2 Claude | Do the plugin hooks, env and MCP behave as documented? | Install a generated plugin. Script these scenarios: prompt, tool, permission prompt, AskUserQuestion, Esc interrupt, `/clear`, `/compact`, subagent, background task, exit. Check stdio env inheritance, `list_changed` in a live session, `MessageDisplay` lag, `~/.claude/sessions/<pid>.json` and `claude agents --json`, and whether `CLAUDE_CODE_RESUME_INTERRUPTED_TURN` works in the TUI. | Correct state on every scenario with hooks + OSC. Any miss gets a documented workaround. |
| S3 plugins | Is the iframe/worker sandbox tight and alive? | Run an evil plugin that tries Tauri `invoke`, `__TAURI_INTERNALS__`, parent DOM access and off-allowlist fetch, on each OS (Windows is the risk). Check that worker timers keep running with the window hidden or minimized. Measure tool call round trip. | No escape on any OS. Worker responds within 50 ms while hidden. Fail → move workers to WASM (D9 rival) or a separate `WebviewWindow`. |
| S4 index | How fast is ingest? | Full index of the 1,457 local transcripts, then append-tail latency. | Full scan time recorded. Incremental update < 500 ms. |

---

## 7. Risks

- **Overlap.** Wrapping the real TUI, auto-discovery and history are already done by Herdr (42k stars, funded), herdr-gpui (a Rust GUI for Herdr), Orca, Superset and cmux. The open slot is plugins that ship GUI panels plus agent tools behind one auto-registered endpoint. Claude Code's own mods already add panes and tools inside the TUI, so Wings plugins should focus on UI outside the terminal and on cross-session views.
- **Claude Code drift.** The transcript format is internal, hook fields change, and the MCP runtime changes. The adapter layer (D5), the version log and a nightly test against the latest `claude` contain the damage.
- **Linux webview.** WebKitGTK GPU problems (NVIDIA, Wayland Error 71) are the most likely user complaint. Tauri 3's CEF runtime is the contingency once it leaves alpha.
- **Tauri child webviews** (`unstable`) have open input bugs on macOS and don't work on Wayland. Don't use them. Plugins use iframes or separate `WebviewWindow`s.

---

## 8. Open questions for you

1. **License.** Apache-2.0 would let us reuse Herdr's code (Apache-2.0) such as the Claude detection manifest and process inspection, plus MIT parsers from cc-switch and Orca. Recommended.
2. **Sessions outside Wings.** Should Wings also track live Claude sessions you start in other terminals? The hooks would fire there too, gated by a setting, not the pane env. History covers them either way.
3. **Plugin runtime (D9).** Is web-first (TS only, at first) fine for plugin authors, or do you want multi-language logic from the start, which means WASM?
