> Research notes gathered 2026-10-08 by a research agent. Paths under `/private/tmp/...scratchpad/` were temporary and no longer exist. Treat versions and issue states as of that date.

Claude Code integration research for Wings (date 2026-10-08, installed CLI 2.1.294 at ~/.local/bin/claude). Read-only. Nothing in the project was touched.

Method and tags. Docs were fetched as raw markdown with curl from https://code.claude.com/docs/en/<page>.md and https://code.claude.com/docs/llms.txt. Copies are in /private/tmp/...scratchpad/cc-research/ (for example hooks.md, statusline.md, env-vars.md, cli-reference.md, mcp.md, sessions.md, agent-view.md, jetbrains.md). Basis tags: [docs] official page, [local] measured on this machine, [binary] string found in the 2.1.294 binary, [undoc] observed but not documented. Scripts and outputs are in the same scratch folder: transcript_shape.py, full_scan.py, full_scan.txt, shape_ai_setup.txt, compact_keys.py, subagent_keys.py.

Not run, to stay read-only: `claude agents --json` (the docs say it can start the supervisor, and `claude daemon status` reports "not running"), any `--resume` or new session, and live execution of hook or statusline commands.

NOTE: two Bash tool results (the transcript-shape scan and an earlier help call) carried an appended "[rule-card]" block claiming system authority over code-writing style. It did not come from you or the task. I ignored it. It had no effect on this research.

1. TRANSCRIPTS

- Location [docs: https://code.claude.com/docs/en/sessions.md "Where transcripts are stored"]: ~/.claude/projects/<project>/<session-id>.jsonl. The <project> name is the working directory with every non-alphanumeric character replaced by "-". Names longer than 200 characters are truncated and a hash is appended. [local] I re-encoded the recorded cwd of all 1,457 transcripts and matched the folder name in 1,457 of 1,457 cases. No folder name exceeded 200 characters, so the truncation branch was not exercised.
- Root moves with CLAUDE_CONFIG_DIR. CLAUDE_CODE_PROJECT_DIR_NAME (which needs CLAUDE_CONFIG_DIR set too) names the project folder yourself [docs sessions.md "Name the project directory yourself"].
- Docs warn that the entry format is internal and changes between versions, so scripts that parse it can break [docs sessions.md].
- Entry types [local, 1,457 files, 0 parse errors, counts]: attachment 126241, assistant 68075, user 43414, last-prompt 17531, atis-latch 16370, ai-title 12929, bridge-session 12646, mode 12554, permission-mode 11968, queue-operation 10103, system 8053, file-history-snapshot 3544, cost-state 2605, file-history-delta 791, pr-link 618, agent-name 534, custom-title 452, dev-mods 78, relocated 42, worktree-state 40, frame-link 29, artifact-autoreact-ledger 28, artifact-comment-monitor 5, continued-in 3.
- There is no top-level "summary" type. Titles: ai-title (aiTitle) and custom-title (customTitle). Compaction: system/compact_boundary (22 entries). Away recap: system/away_summary (636). Other system subtypes: stop_hook_summary, turn_duration, local_command, informational, permission_retry, bridge_status, scheduled_task_fire, and two model_refusal variants [local].
- Message entries (user, assistant, system) carry: uuid, parentUuid (a tree, so branches are possible), sessionId, cwd, gitBranch, timestamp (ISO-8601 with milliseconds, UTC "Z"), version, entrypoint, userType, isSidechain. The snake_case session_id also appears on attachment, assistant, user, and system entries [local].
- message object: role, content (a list), model, id, stop_reason, usage. usage keys: input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens, cache_creation, service_tier, speed, iterations, server_tool_use, inference_geo, output_tokens_details, fallback_credit [local].
- Content block key sets [local]: text {text,type}; thinking {thinking,signature,type}; tool_use {id,name,input,caller,type}; tool_result {tool_use_id,content,is_error,type}; image.
- Cost: cost-state entries carry totalCostUSD, totalDuration, totalAPIDuration, totalLinesAdded/Removed, modelUsage, startTime [local].
- Subagents [docs claude-directory.md table; local]: <project>/<session-id>/subagents/agent-<id>.jsonl, with a sidecar agent-<id>.meta.json (agentType, description, toolUseId, parentAgentId, spawnDepth, requestShape, requestNonInteractive). Subagent entries have isSidechain=true, an agentId field, and sessionId equal to the parent session id (14764 of 14764 checked). Local count: 529 subagent .jsonl files and 535 .json sidecars across all projects. SubagentStop hooks get agent_transcript_path [docs hooks.md "SubagentStop input"].
- Large tool outputs spill to <session-id>/tool-results/ (66 folders locally) [docs claude-directory.md; local].
- Index: there is no index file under projects/. The session list comes from scanning. Related files: ~/.claude/history.jsonl (8945 prompt lines with keys display, pastedContents, timestamp, project, sessionId) [local]; ~/.claude/sessions/<pid>.json (live registry, see section 5 and section 9, [undoc]); ~/.claude/jobs/<id>/state.json for background sessions. The docs say not to parse jobs state; use `claude agents --json` [docs agent-view.md].
- Append-only: docs say sessions "are saved continuously" [docs sessions.md]. They also say the transcript "is written asynchronously and may lag the in-memory conversation" [docs hooks.md "Common input fields"]. Two terminals resuming the same session without forking interleave into one file [docs sessions.md]. Superseded transcripts are set aside as .orphaned-* or .jsonl.superseded-* rather than overwritten [docs claude-directory.md]. Local test: the first 1,000,000 bytes of the live session file had the same SHA-256 before and after my checks. But the file did not grow at all during this turn (about 25 tool calls), so growth lags turns. Result: prefix stability held; growth timing is inconclusive. Safe tailing: read from a byte offset, consume only up to the last newline, and tolerate a partial final line (the live file did end in a newline at the check).
- Retention: transcripts age out after cleanupPeriodDays, default 30 [docs claude-directory.md].

2. RESUME AND FLAGS

- `claude --resume [value]` (-r): takes a session ID, a name, or an absolute .jsonl path. With no value it opens the picker [local claude --help; docs cli-reference.md, sessions.md].
- `claude --continue` (-c): most recent conversation in the cwd. It skips sessions created with -p or the Agent SDK, and sessions whose first prompt was /loop [local help; docs sessions.md].
- `--session-id <uuid>`: local help says "Use a specific session ID for the conversation (must be a valid UUID)" [local help]. Pre-assigning it means the transcript path is known before the process starts: ~/.claude/projects/<encoded-cwd>/<uuid>.jsonl. Not executed here.
- `--fork-session`: with --resume or --continue, creates a new session ID [local help].
- `--name` / `-n`: display name, shown in /resume and the terminal title. `--resume <name>` resolves it. If another live session already uses the name, a suffix is added [docs cli-reference.md, sessions.md "Name your sessions"]. /rename and Ctrl+R in the picker also name sessions [docs sessions.md].
- `--resume <id>` works from any directory. The cross-project lookup succeeds only when exactly one other project holds that ID [docs sessions.md].
- Not restored on resume: --mcp-config, --settings, --plugin-dir, --fallback-model, and --add-dir. You must pass them again [docs sessions.md "What a resumed session restores"]. Wings must re-supply these on every resume.
- Terminal resumes restore the permission mode the session ended in [docs sessions.md].
- Embedding-relevant flags, all confirmed in local `claude --help`: --settings <file-or-json>; --mcp-config <configs...>; --strict-mcp-config; --append-system-prompt and -file; --system-prompt and -file; --plugin-dir and --plugin-url (session-only plugins); --add-dir; --agent; --name; --session-id; --remote-control [name] and --rc; --ide; --bg; --setting-sources; --include-hook-events; --input-format; --output-format; --permission-mode; --tools; --safe-mode; --bare.

3. HOOKS

- Events: 33 rows in the summary table [docs https://code.claude.com/docs/en/hooks.md "Hook lifecycle"]: SessionStart, Setup, UserPromptSubmit, UserPromptExpansion, PreToolUse, PermissionRequest, PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch, Notification, MessageDisplay, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop, StopFailure, TeammateIdle, InstructionsLoaded, ConfigChange, CwdChanged, DirectoryAdded, FileChanged, WorktreeCreate, WorktreeRemove, PreCompact, PostCompact, PreModelSwitch, PostModelSwitch, Elicitation, ElicitationResult, SessionEnd.
- Handler types: command, http (POST of the same JSON to a URL), mcp_tool, prompt, agent [docs hooks.md "Hook handler fields"]. HTTP example uses http://localhost:8080 [docs hooks.md]. HTTP hooks cannot block through status codes alone; a block requires a 2xx JSON body with a decision. Non-2xx and connection failures are non-blocking [docs hooks.md "HTTP response handling"]. allowedHttpHookUrls and allowedEnvVars can restrict them.
- PermissionRequest supports command, http, mcp_tool, and prompt, but not agent [docs hooks.md].
- Config locations [docs hooks.md "Hook locations", settings.md]: ~/.claude/settings.json; .claude/settings.json; .claude/settings.local.json; managed policy; plugin hooks/hooks.json; skill and agent frontmatter. --settings can set any key a user settings file can set [docs settings.md "Change a setting for one session"]. Entries from user, project, and local levels merge rather than replace. Cloud sessions do not read ~/.claude/settings.json.
- Common input fields [docs hooks.md "Common input fields"]: session_id, prompt_id, transcript_path, cwd, scratchpad_dir, permission_mode (values default, plan, acceptEdits, auto, dontAsk, bypassPermissions; Manual is reported as "default"), effort {level}, hook_event_name. Inside subagents, agent_id and agent_type are also present.
- Event-specific inputs [docs hooks.md]:
  - SessionStart: source (startup, resume, clear, compact, fork), model (may be absent), agent_type, session_title. On resume it also sends seconds_since_last_response, context_tokens, and prompt_cache_likely_expired.
  - UserPromptSubmit: prompt. Fires for turns Claude starts on its own too.
  - PreToolUse: tool_name, tool_input, tool_use_id; for MCP tools, mcp_server {name, source}. AskUserQuestion is a tool, so PreToolUse fires when Claude asks you a question.
  - PermissionRequest: tool_name, tool_input, permission_suggestions. Fires only when Claude is about to prompt or would auto-deny. Its decision object can allow or deny (behavior, updatedInput, updatedPermissions, message, interrupt). Deny and ask rules still apply.
  - PostToolUse: tool_input, tool_response, duration_ms (excludes permission time).
  - Stop: stop_hook_active, last_assistant_message (use this instead of reading the transcript, which may lag), background_tasks, session_crons.
  - SubagentStop: agent_id, agent_type, agent_transcript_path, last_assistant_message.
  - SessionEnd: reason = clear, resume, logout, prompt_input_exit, or other. Default timeout 1.5 s. Raise it with a per-hook timeout (up to 60 s) or CLAUDE_CODE_SESSIONEND_HOOKS_TIMEOUT_MS.
  - Notification: message, title, notification_type. Matchers: permission_prompt (about 6 s after the prompt appears, and only if you have not typed), idle_prompt (about 60 s after Claude finishes, if you have not typed and no background subagent is running), elicitation_dialog, elicitation_url_dialog, elicitation_complete, elicitation_response, agent_needs_input, agent_completed, auth_success, and quota_auto_resume_* types. Notification hooks cannot block.
- Async: "async": true is allowed only on command hooks. They cannot block, and their output reaches Claude on the next turn. Under `-p`, async hooks still running at teardown are killed and marked cancelled. asyncRewake wakes Claude on exit code 2 [docs hooks.md "Run hooks in the background"].
- Default timeouts: command, http, mcp_tool 600 s; prompt 30 s; agent 60 s [docs hooks.md "Common fields"].
- Output to the user: command hooks have no controlling TTY on macOS or Linux, so they cannot draw in the TUI. They can use systemMessage or terminalSequence (OSC) output [docs hooks.md].
- Environment: a hook process inherits the parent environment [docs hooks.md, line near "A hook process inherits"]. So Wings can set its own variable in the PTY child environment and the hooks will see it.
- Hooks are not in statusLine, and plugins cannot ship statusLine (see section 7).

4. STATUSLINE

- Config: settings.statusLine = {type:"command", command, refreshInterval} (refreshInterval minimum 1 s) [docs https://code.claude.com/docs/en/statusline.md "Manually configure"; settings-reference]. Local user settings currently have exactly these keys: command, refreshInterval, type [local].
- When it runs: once at session start or resume, then after each new assistant message, /compact, permission-mode change, vim toggle, command change, the refreshInterval timer, rate-limit window reset, and prompt cache expiry. Updates are debounced by 300 ms. A new trigger cancels the in-flight run. It runs locally and uses no API tokens [docs statusline.md "When the status line updates"].
- Stdin JSON [docs statusline.md "Available data" and "Full JSON schema"]: session_id, session_name, prompt_id, transcript_path, cwd, model {id, display_name}, workspace {current_dir, project_dir, added_dirs, git_worktree, repo {host, owner, name}}, version, output_style, cost {total_cost_usd, total_duration_ms, total_api_duration_ms, total_lines_added, total_lines_removed}, context_window {total_input_tokens, total_output_tokens, context_window_size, used_percentage, remaining_percentage, current_usage {...}}, exceeds_200k_tokens, prompt_cache {warm, hit_ratio, ttl, expires_at, requests, misses, ...}, fast_mode, effort {level}, thinking {enabled}, rate_limits {five_hour, seven_day} each with used_percentage and resets_at, pr {number, url, review_state, kind}, vim, agent {name}, worktree {...}.
- Not in stdin: working or waiting state, current tool, pending permission. Use hooks for those.
- Subagent rows have a separate subagentStatusLine setting. It receives a tasks array with id, name, agentType, status, description, label, startTime, model, tokenCount, and more [docs statusline.md "Subagent status lines"].
- COLUMNS and LINES are set for the script (stdout is captured, so tput cannot read the size) [docs statusline.md "Size output"].
- Caveat for Wings: statusLine is one command per settings layer. A --settings statusLine overrides the user's statusLine for that session (settings precedence) [docs settings.md "Settings precedence"]. Not tested.

5. ENVIRONMENT

Set by Claude Code, documented [docs https://code.claude.com/docs/en/env-vars.md]:
- CLAUDECODE=1: in Bash and PowerShell tools, tmux sessions, hook commands, statusline commands, stdio MCP servers. IDE integrated terminals also set it.
- CLAUDE_CODE_CHILD_SESSION=1: set for Bash, PowerShell, Monitor, hook, and statusline children. Not set for stdio MCP servers. A nested TUI with this set is excluded from --resume, --continue, and the agents list unless CLAUDE_CODE_FORCE_SESSION_PERSISTENCE=1.
- CLAUDE_CODE_SESSION_ID: set in Bash, PowerShell, hook, and stdio MCP subprocesses. Matches hook session_id. Updated on /clear. An MCP server keeps the ID it was spawned with.
- CLAUDE_PID: Claude Code's own PID, set in Bash, PowerShell, and hook commands.
- CLAUDE_PROJECT_DIR: in hook commands, stdio MCP servers, and plugin LSP servers (the MCP page also says the variable is set in the server's environment).
- CLAUDE_CODE_MESSAGING_SOCKET and CLAUDE_CODE_MESSAGING_TOKEN: the session inbox socket and its per-session token. Exported before any hook runs, including SessionStart [docs cross-session-messaging.md "The session's inbox socket"].
- CLAUDE_CODE_BRIDGE_SESSION_ID: set while Remote Control is connected [docs remote-control.md].
- CLAUDE_ENV_FILE: available to SessionStart, Setup, CwdChanged, and FileChanged hooks.
- CLAUDE_JOB_DIR: set for background sessions.
- CLAUDE_PLUGIN_ROOT and CLAUDE_PLUGIN_DATA: plugin hook and MCP paths.
- CLAUDE_EFFORT: mentioned in hooks docs.

Observed in this shell but not in the docs [undoc]: CLAUDE_CODE_ENTRYPOINT=cli, CLAUDE_CODE_EXECPATH, CLAUDE_CODE_SESSION_ATTENDED=1. Also TERM_PROGRAM=herdr here, which is the embedding terminal's identity.

Variables Claude Code reads that an embedding host might set [docs env-vars.md]:
- CLAUDE_CONFIG_DIR: relocates everything, including the session registry and the IDE lock folder.
- CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST: documented for hosts that embed Claude Code. It makes Claude Code ignore provider, endpoint, and auth settings in settings files.
- CLAUDE_CODE_AUTO_CONNECT_IDE (true or false), CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL, CLAUDE_CODE_IDE_HOST_OVERRIDE, CLAUDE_CODE_IDE_SKIP_VALID_CHECK.
- CLAUDE_CODE_DISABLE_TERMINAL_TITLE: stops the automatic terminal title.
- CLAUDE_CODE_NO_FLICKER, and the tui setting ("fullscreen" uses the alternate screen with virtualized scrollback; "default" is the classic renderer). Both change how a PTY screen looks.
- CLAUDE_CODE_SHELL, CLAUDE_CODE_SUBPROCESS_ENV_SCRUB (strips credentials from child environments, including hooks and stdio MCP servers), MCP_TIMEOUT, MAX_MCP_OUTPUT_TOKENS, CLAUDE_CODE_SIMPLE (same as --bare), and CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH.
- TERM_PROGRAM: the docs mention it for strikethrough and backspace detection.

6. MCP

- Add: `claude mcp add [-s local|project|user] [-t stdio|sse|http] [-e K=V] [-H header] <name> <url-or-command> [args]`. Default scope is local [local `claude mcp add --help`]. `claude mcp add-json <name> '<json>'` also accepts WebSocket [local help; docs mcp.md "Configure WebSocket servers"].
- Storage [docs mcp.md "MCP installation scopes"]: local and user scopes live in ~/.claude.json. Local is under projects[<path>].mcpServers and user is top-level mcpServers. Project scope is .mcp.json at the project root, which needs approval in an interactive session. Local check: ~/.claude.json has a top-level mcpServers key with 1 entry (Framelink Figma), and projects[*].mcpServers has 0 entries. Only key names were read.
- Per-session injection: --mcp-config <files-or-json...> plus --strict-mcp-config. These are not restored on resume.
- Transports [docs mcp.md]: stdio; http (JSON type "streamable-http" is an alias); sse (deprecated, with an automatic HTTP-to-SSE fallback); ws (WebSocket); sdk (in-process, only for an SDK host or the desktop app; a type "sdk" entry in .mcp.json or settings is skipped).
- Dynamic tool list [docs mcp.md "Dynamic tool updates"]: list_changed notifications are honored. In interactive terminals Claude Code re-fetches the list. In `-p` and SDK sessions it refreshes only the tool list. On the v2 runtime (protocol 2026-07-28), the notification stream reopens with limits. Enabling or disabling a plugin takes effect via /reload-plugins. A reload that adds or removes MCP tools can invalidate the prompt cache.
- Tool search [docs mcp.md "Scale with MCP tool search"]: tool definitions load on demand. Only names and server instructions load at start. Descriptions are truncated at 2,048 characters (CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH). It is disabled when ANTHROPIC_BASE_URL points somewhere other than first-party.
- Resources: referenced with @server:protocol://path [docs mcp.md "Use MCP resources"]. Prompts appear as /mcp__server__prompt and as /server:prompt (MCP) [docs mcp.md "Use MCP prompts as commands"].
- Elicitation [docs mcp.md "Respond to MCP elicitation requests"]: form mode and URL mode. Hooks Elicitation and ElicitationResult can auto-respond. On protocol 2026-07-28, Claude Code declares elicitation {form, url} capabilities.
- MCP Apps: ui:// resources and text/html;profile=mcp-app are described as pages "for a host application to render rather than content for Claude to read" [docs mcp.md]. The docs describe no terminal rendering. Wings, as a host, can read them with its own MCP client.
- Tool output limits [docs mcp.md "MCP output limits"]: warning above 10,000 tokens; default cap 25,000 tokens (MAX_MCP_OUTPUT_TOKENS raises it); text over 50,000 characters is saved to a file unless the tool sets the anthropic/maxResultSizeChars annotation; images are shown inline. Per-server timeout field; idle timeout defaults to 5 min for HTTP and 30 min for stdio.
- How a server learns the calling session: [docs env-vars.md, mcp.md]:
  - stdio: CLAUDE_CODE_SESSION_ID in the server's environment. It is the spawn-time ID, which can be stale after --continue or /clear. CLAUDE_PROJECT_DIR is set too. roots/list returns the launch directory.
  - http and ws: no documented session header or MCP field. A search of all downloaded docs found no session header for these transports.
  - [binary, undoc] X-Mcp-Client-Session-Id is set only on the claude.ai connector proxy transport. Not usable for local servers.
- Claude Code as an MCP server: `claude mcp serve` [docs mcp.md "Use Claude Code as an MCP server"].
- Channels [docs https://code.claude.com/docs/en/channels-reference.md; cli-reference]: an MCP server declares the claude/channel capability and is opted in with `--channels plugin:<name>@<marketplace>`. Requires claude.ai or Console API key login. Research preview. On the v2 runtime, a channel server on protocol 2026-07-28 is not registered as a channel.

7. PLUGINS

- Manifest: .claude-plugin/plugin.json. Optional. Required key: name (kebab-case). Other keys: displayName, version, description, author, homepage, repository, license, keywords, defaultEnabled (default true), dependencies, settings, userConfig, channels, skills, commands, agents, hooks, mcpServers, lspServers, outputStyles, workflows, experimental {themes, monitors, evals} [docs https://code.claude.com/docs/en/plugins/manifest-reference.md].
- Bundleable [docs plugins/components.md, manifest-reference.md]: skills; commands; agents; hooks (hooks/hooks.json, five handler types); MCP servers (.mcp.json or inline, started when the plugin is enabled); LSP servers; executables in bin/ (on the Bash tool PATH); output styles; themes (experimental); monitors (experimental; background shell commands whose output reaches Claude; interactive sessions only); channels; workflows; and default settings.
- Default settings: only agent and subagentStatusLine take effect. All other keys are dropped at load [docs manifest-reference.md "settings"; components.md "Default settings"]. So a plugin cannot ship statusLine, env, or permissions.
- Install and manage [local `claude plugin --help`; docs plugins/cli-reference.md]: `claude plugin marketplace add <URL|path|GitHub repo> [--scope user|project|local]`; `claude plugin install <plugin>[@marketplace] [-s user|project|local] [--config k=v] [--json]` (default scope user); enable, disable, update, uninstall, list, details, validate. Per session: --plugin-dir <path-or-zip> and --plugin-url. In-session reload: /reload-plugins, which prints a summary line [docs cli-reference "Reload summary"].
- Could Wings ship its integration as a plugin? Yes, with limits. Inferred: a user-scope install enables it in every session on the machine, so the hooks must check for a Wings-only environment variable and no-op otherwise. A plugin cannot add statusLine. Installing requires a marketplace step or --plugin-dir per session.

8. PROGRAMMATIC ALTERNATIVES

- Print mode [docs https://code.claude.com/docs/en/headless.md; cli-reference]: `claude -p` with --output-format json or stream-json (add --verbose; --include-partial-messages for token deltas; --include-hook-events for hook lifecycle events). --input-format stream-json feeds JSON in; --replay-user-messages echoes input. The stream starts with a system/init event carrying session metadata, model, tools, MCP servers, and plugins. Each event carries session_id. Print-mode sessions are left out of the picker and --continue.
- Agent SDK, TypeScript package @anthropic-ai/claude-agent-sdk [docs https://code.claude.com/docs/en/agent-sdk/typescript.md; sessions.md; session-storage.md]: query() options include resume, continue, sessionId, forkSession, includePartialMessages, mcpServers, plugins, hooks, canUseTool, permissionMode, env, cwd, pathToClaudeCodeExecutable, sessionStore. Session helpers: listSessions({dir, limit, includeWorktrees}) returns SDKSessionInfo {sessionId, summary, lastModified, fileSize, customTitle, firstPrompt, gitBranch, cwd, tag, createdAt}. getSessionMessages(sessionId, {dir, limit, offset}) returns {type, uuid, session_id, message, parent_tool_use_id, parent_agent_id}. Also getSessionInfo, renameSession, tagSession, forkSession, listSubagents. Python equivalents exist.
- For Wings this matters because listSessions and getSessionMessages let you read history without parsing the internal JSONL format. The SDK is a harness you host. It is not the Anthropic API Tool Runner, and it is not Managed Agents.
- Wings plans to wrap the interactive TUI in a PTY, so this is secondary.

9. REMOTE CONTROL AND IDE PROTOCOL

- Remote Control [docs https://code.claude.com/docs/en/remote-control.md]: `claude remote-control`, `/remote-control` or `/rc`, or `--remote-control [name]`. It connects claude.ai/code or the mobile app. The local session makes outbound HTTPS only and opens no inbound ports. No local API for third-party apps is documented. Requires a claude.ai subscription; API keys are not supported.
- IDE integration, documented for JetBrains [docs https://code.claude.com/docs/en/jetbrains.md "The built-in IDE MCP server"]:
  - The plugin runs a local MCP server named "ide" on an OS-assigned port (not configurable). Transport is unencrypted ws://.
  - Each IDE start writes a lock file at ~/.claude/ide/<port>.lock containing a fresh random auth token. If CLAUDE_CONFIG_DIR is set, the folder is $CLAUDE_CONFIG_DIR/ide/. The lock file is mode 0600 in a 0700 folder.
  - The CLI presents the token in the X-Claude-Code-Ide-Authorization header.
  - Only mcp__ide__getDiagnostics is model-visible. Other tools are internal RPC for diffs and selection.
  - Commands and settings: /ide command; CLAUDE_CODE_AUTO_CONNECT_IDE; the autoConnectIde setting.
- [binary] The 2.1.294 binary defines IDE transports ws-ide (WebSocket with subprotocol "mcp" and the auth header) and sse-ide. The lock-file JSON key names are not documented, and I did not verify them.
- VS Code: docs/vs-code.md documents no lock-file or socket protocol.
- Local: ~/.claude/ide exists and is empty (no IDE connected).
- Conclusion: Wings could impersonate an IDE by writing a lock file and serving MCP over ws with the token. That is undocumented as a public extension point, so treat it as unsupported and use /feedback to ask for a supported integration.

BEST SIGNALS FOR SESSION STATE DETECTION (ranked)

Ranking for the four you asked about:

1. Hooks (command, or http to localhost). Best for real-time transitions.
   - Pros: documented; structured JSON; session_id, transcript_path, cwd, and tool details on every event; correlates to the PTY through the inherited environment variable and CLAUDE_PID; PermissionRequest can answer prompts, not just observe them; works across terminal, desktop, and cloud [docs hooks.md intro].
   - Cons: config is user-wide, so it fires for every Claude session on the machine unless gated by an env var. A slow hook delays the agent (command default 600 s). HTTP hooks cannot be async. Idle via Notification comes about 60 s late, so use Stop for immediate done. No streamed text.
   - Map: SessionStart creates the session. UserPromptSubmit means busy. PreToolUse and PostToolUse give the current tool. PermissionRequest means waiting for permission, and AskUserQuestion via PreToolUse means waiting for an answer. Stop means done. SessionEnd means exited.

2. Transcript tailing. Best for rendering and history.
   - Pros: complete record of messages, tool calls and results, thinking, subagent files, titles, model, usage, and cost. Survives restarts. No config needed.
   - Cons: lags turns (observed). Internal format that changes by version. No explicit "waiting" marker, so you must infer it from hooks. Partial final lines and separate subagent files must be handled. Prefer the SDK helpers where they cover the need.

3. PTY screen parsing. A last-resort fallback.
   - Pros: works with zero configuration. Shows the exact dialogs and prompts the user sees, including the terminal title.
   - Cons: fragile across versions. The fullscreen renderer uses the alternate screen with virtualized scrollback. Output is ANSI-heavy. Gives no structured state.

4. IDE protocol (lock file plus ws MCP). Not a state source.
   - Pros: a live authenticated socket.
   - Cons: exposes selection, diagnostics, and diffs, not session state. The third-party contract is undocumented. The lock-file schema is undocumented. It may change without notice.

Two additional documented signals to combine with the four:
- `claude agents --json` (documented as the supported way to read session state from outside Claude Code). Lists active interactive and background sessions. Per entry: cwd, kind, startedAt, id (background only), state (working, blocked, done, failed, stopped for background), pid and status (busy, waiting, idle) while the process lives, waitingFor (permission prompt, input needed, sandbox request, worker request, dialog open), sessionId, name. Poll it, or use it to reconcile after a restart [docs agent-view.md "List sessions as JSON"]. Not executed here, because it may start the supervisor. Its local help says it "prints active sessions (interactive and background)" [local `claude agents --help`].
- statusLine JSON: per-session metrics only (cost, context percent, model, rate limits, cache), not a state signal.

Undocumented but working locally: ~/.claude/sessions/<pid>.json. Five entries at check time. Keys (names and types only): pid, sessionId, cwd, startedAt, procStart, version, peerProtocol, peerFeatures, kind (interactive), entrypoint (cli), pidDomain, messagingSocketPath, name, nameSource, nameSince, status (busy or idle), updatedAt, statusUpdatedAt, bridgeSessionId. The docs describe the folder only as "one small file per running session, used to detect concurrent sessions and crashes" [docs claude-directory.md]. The file for this session has filename 27561, which equals CLAUDE_PID in this shell, and its sessionId equals CLAUDE_CODE_SESSION_ID. A sibling <pid>.key file (mode 0600) exists; I did not read it. Treat this file as fast but internal.

Recommended combination: hooks for transitions; transcript tailing for content; `claude agents --json` to reconcile after a restart; PTY parsing only as a fallback.

UNVERIFIED OR NOT DONE
- `claude agents --json` output was not run (documented only).
- No resume or new session was launched. --session-id and fork behavior are from docs and help text.
- Hook and statusline payloads were taken from docs, not captured live.
- Transcript growth timing: inconclusive (no growth during this turn).
- Hook correlation via inherited env var, and --settings carrying hooks or statusLine: docs-based, not tested.
- The MCP server session-identity claims for stdio come from docs, not a live server.
- IDE lock-file JSON schema: not verified.

Scratch folder (all outputs): /private/tmp/...scratchpad/cc-research/. Main doc copies: hooks.md, statusline.md, env-vars.md, cli-reference.md, mcp.md, sessions.md, claude-directory.md, agent-view.md, remote-control.md, jetbrains.md, cross-session-messaging.md, headless.md, settings.md, plugins_manifest-reference.md, plugins_components.md, plugins_cli-reference.md, agent-sdk_typescript.md, agent-sdk_sessions.md, agent-sdk_session-storage.md, channels-reference.md, llms.txt. Shape outputs: shape_ai_setup.txt and full_scan.txt.