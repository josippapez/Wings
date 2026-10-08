> Research notes gathered 2026-10-08 by a research agent. Paths under `/private/tmp/...scratchpad/` were temporary and no longer exist. Treat versions and issue states as of that date.

# Wings plugin system and single MCP endpoint: evidence report (as of 2026-10-08)

Basis tags: [read] = I opened the page or file this session; [measured] = I ran a command or API call; [inferred] = my reasoning, not a fact; unverified = I could not settle it. Raw downloads are in `/private/tmp/...scratchpad/plugin-research/`, and I only read them. The `~/Desktop/wings` directory is empty, so there is no Wings code to check against.

## Headline findings that change the design

1. **The current MCP spec is `2026-07-28` and it is stateless.** There is no `initialize`, no `Mcp-Session-Id` and no HTTP GET stream. List-change notifications now arrive on a `subscriptions/listen` POST stream. [read]
2. **Claude Code speaks both eras.** Which one depends on its "v1" or "v2" MCP runtime, picked per launch. Wings' MCP server must therefore be dual-era. rmcp 3.5.1 is. [read]
3. **Claude Code does not render MCP Apps UI.** It hides `ui://` resources from `@` suggestions and from the resource-list tool. Wings has to host plugin UI itself. [read]
4. **Claude Code's transcript JSONL format is documented as internal and changing.** Hooks are the documented, structured event source. Wings should normalize events into its own versioned schema. [read]
5. **Figma abandoned an in-JS sandbox after a vulnerability.** It moved to a C JavaScript VM compiled to WebAssembly. [read]
6. **Extism's latest release pins an unsupported Wasmtime.** Extism 1.30.0 depends on `wasmtime ^43`, a non-LTS release that is outside its 2-month support window. [measured, with the support window from the Wasmtime policy]

---

## 1. MCP specification and MCP Apps

### 1.1 Version and status

- The current version is **2026-07-28**. The revision page says "The current protocol version is 2026-07-28", and the sources list 2025-11-25 as the previous revision. [read] https://modelcontextprotocol.io/specification/versioning and https://modelcontextprotocol.io/specification/2026-07-28/changelog
- Major changes in 2026-07-28 [read, same changelog]:
  - Protocol-level sessions and `Mcp-Session-Id` are removed (SEP-2567).
  - The `initialize` handshake is removed. Every request carries protocol version and client capabilities in `_meta` (SEP-2575).
  - `server/discover` is mandatory.
  - `subscriptions/listen` replaces the GET stream and `resources/subscribe`.
  - Tasks moved out of core into the extension `io.modelcontextprotocol/tasks`.
  - Server-initiated requests (sampling, elicitation, roots) are replaced by Multi Round-Trip Requests: a server returns `input_required` and the client retries with `inputResponses`.
  - SSE resumability (`Last-Event-ID`) is removed.
- The same changelog marks Roots, Sampling and Logging as **Deprecated**, with earliest removal in the first revision released on or after 2027-07-28. https://modelcontextprotocol.io/specification/2026-07-28/deprecated [read]
- Dual-era servers are allowed: a modern request is served statelessly, and an `initialize` request selects legacy semantics. The compatibility matrix is on https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning [read]. A legacy client against a modern-only server fails.

### 1.2 Gateway-relevant features

| Feature | 2026-07-28 | Source |
|---|---|---|
| Streamable HTTP | One endpoint accepting POST. Each request is its own POST. The reply is JSON or a request-scoped SSE stream. GET stream and sessions removed. Servers MUST validate `Origin` and SHOULD bind to 127.0.0.1. | https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http [read] |
| `tools/list_changed` | Delivered only to clients that opened `subscriptions/listen` with `toolsListChanged: true`. The server acknowledges first. State is not resumed across reconnects. | https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions [read] |
| Tool list variance | `tools/list` MUST NOT vary per connection. It MAY vary by the authorization presented on the request. This is the legal route to per-session tool sets. | https://modelcontextprotocol.io/specification/2026-07-28/server/tools [read] |
| Cross-call state | Use explicit server-minted handles passed as tool arguments. | same tools page [read] |
| Tool names | SHOULD be 1-128 characters from `A-Za-z0-9_-.`. Aggregators SHOULD prefix with a server identifier. `serverInfo.name` is not guaranteed unique. | same tools page [read] |
| Structured output | `outputSchema` may be any JSON Schema 2020-12 keywords. `structuredContent` may be any JSON value (SEP-2106). | changelog [read] |
| Caching | Required `ttlMs` and `cacheScope` on list results. | https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/caching [read] |
| Resources and prompts | Still core. Resource-not-found code is now -32602. | changelog [read] |
| Elicitation | Still exists, but delivered through the Multi Round-Trip pattern. | changelog [read] |
| Sampling | Deprecated. | deprecated registry [read] |
| Tasks | Official extension `io.modelcontextprotocol/tasks`, polling via `tasks/get` and `tasks/update`. | https://modelcontextprotocol.io/extensions/tasks/overview [read] |
| Skills over MCP | Final extension `io.modelcontextprotocol/skills`. Host support "still being implemented". | https://modelcontextprotocol.io/extensions/skills/overview [read] |

- In **2025-11-25**, sessions are optional. A server may issue `MCP-Session-Id` at initialize, and clients must echo it. A 404 means "start a new session". https://modelcontextprotocol.io/specification/2025-11-25/basic/transports [read]
- Tasks in 2025-11-25 were "experimental". https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks [read]

### 1.3 Claude Code as the MCP client

All from https://code.claude.com/docs/en/mcp unless noted [read]. npm shows `@anthropic-ai/claude-code` latest 2.1.293 and stable 2.1.285 [measured].

- **Runtimes.** There are two MCP client runtimes. v1 is built on TypeScript SDK 1.x. v2 is built on SDK 2.0 and adds 2026-07-28.
  - v2 is used on 2.1.232+ when feature flags are fetched.
  - v2 is the default on 2.1.274+ in specific cases: Bedrock, Foundry or Vertex, a Claude apps gateway, or telemetry/flags disabled.
  - Env vars `MCP_SDK_GENERATION` and `MCP_PROTOCOL_NEGOTIATION` override this.
- **Dynamic tools.** `list_changed` is honored. An interactive session refreshes tools, prompts and resources. `-p` and the SDK refresh tools only. On v2, the stream reopens with limits (3 reopens if it closes within 10 s; after 5 reopens per hour it waits about 6 h).
- **Prompts and resources.** MCP prompts become `/mcp__server__prompt` commands. Resources are `@`-mentionable.
- **MCP Apps.** "MCP Apps UI resources are entries with a `ui://` URI or the `text/html;profile=mcp-app` media type: pages for a host application to render." They are hidden from `@` and from the resource-list tool, though reading by URI still works.
- **Elicitation.** Form and URL modes are supported. On 2026-07-28 connections Claude Code declares `elicitation: {form:{}, url:{}}`.
- **Silent areas.** The docs I fetched say nothing about sampling, Tasks, or `structuredContent` handling. unverified.
- **Config injection.**
  - `--mcp-config` accepts files or strings.
  - `--settings` adds settings above user and project and below managed.
  - `${VAR}` expands in `url` and `headers`. Credential-looking variables (`ANTHROPIC_*`, `AWS_*`, `HTTPS_PROXY`, `NPM_TOKEN` and similar) read as empty in remote `url` and `headers`.
  - Project `.mcp.json` servers require an approval prompt in interactive sessions.
  - No env var injects MCP config. [read] https://code.claude.com/docs/en/cli-reference, https://code.claude.com/docs/en/env-vars
- **Naming.**
  - Tools are `mcp__<server>__<tool>`.
  - Plugin-bundled servers become `mcp__plugin_<plugin>_<server>__<tool>`.
  - The Claude API tool-name regex is `^[a-zA-Z0-9_-]{1,128}$`, so no dots even though the MCP spec allows them. https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools [read]
  - Allow rules accept `mcp__<server>__*`. https://code.claude.com/docs/en/permissions [read]
- **Context and timeouts.**
  - Tool search is on by default, so only tool names and server instructions load up front. Descriptions and instructions are truncated at 2,048 characters.
  - The idle timeout is 5 minutes for HTTP and 30 minutes for stdio. Progress notifications reset idle but not the wall-clock `timeout`.
  - A main-conversation call still running after 2 minutes becomes a background task.
  - Stdio servers are not auto-reconnected; HTTP servers are.
- **Channels** (a server pushing events into a session) are research preview, spawned over stdio, and custom ones are not on the allowlist. https://code.claude.com/docs/en/channels-reference [read]. They are not a viable core path.

### 1.4 MCP Apps (SEP-1865)

- **Status.** SEP-1865 is "Final", with extension id `io.modelcontextprotocol/ui`. The ext-apps spec is `2026-01-26` (Stable) plus a draft. SDK `@modelcontextprotocol/ext-apps` is 2.0.3 (2026-09-25) [measured]. https://github.com/modelcontextprotocol/ext-apps and https://modelcontextprotocol.io/extensions/apps/overview [read]
- **Protocol.**
  - A tool declares `_meta.ui.resourceUri`.
  - The host fetches it with `resources/read`.
  - It renders `text/html;profile=mcp-app` in a sandboxed iframe.
  - The view talks JSON-RPC 2.0 over `postMessage`: `ui/initialize`, `ui/notifications/tool-input` and `tool-result`, `ui/message`, `ui/update-model-context`, `ui/open-link`, `ui/notifications/host-context-changed`, among others.
  - `visibility: ["app"]` hides a tool from the model but lets the view call it.
  - Unsupported hosts fall back to the text-only tool.
  https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx [read]
- **Hosts.** The official support matrix lists: Claude (web), Claude Desktop, VS Code Copilot, Microsoft 365 Copilot, Goose, Postman, MCPJam, ChatGPT, Cursor, Archestra.AI, PostHog Code. https://modelcontextprotocol.io/extensions/client-matrix [read]
  - The matrix is community-maintained and does **not** list the Claude Code CLI.
  - Claude's own MCP Apps page shows the example in Claude Desktop and mentions Claude Code only as a place to install the authoring skills. https://claude.com/docs/connectors/building/mcp-apps/getting-started [read]
  - Whether Claude Code Desktop's Code tab renders MCP Apps is unverified.
- **Can Wings render MCP Apps itself? Yes.**
  - `@modelcontextprotocol/ext-apps/app-bridge` is the host SDK. The README says there is no supported host implementation in the repo beyond `examples/basic-host`.
  - `@mcp-ui/client` offers `AppRenderer` and calls itself the recommended SDK for MCP Apps hosts. https://github.com/idosal/mcp-ui [read]
  - The spec requires a web-page host to use a **sandbox proxy on a different origin**, with `allow-scripts` and `allow-same-origin` on the proxy's iframe.
  - The spec's default CSP is: `default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' data:; connect-src 'none'`. Extra origins come from `_meta.ui.csp` and the host must not loosen them.
  - Caveat [inferred]: MCP Apps is anchored on a tool call inside a conversation. Wings needs persistent panels driven by session streams, so Wings would extend the dialect with its own `wings/*` methods.

---

## 2. Rust MCP SDK and existing gateways

### 2.1 rmcp

- **Version.** rmcp **3.5.1**, published 2026-10-05, Apache-2.0, MSRV 1.88, repo https://github.com/modelcontextprotocol/rust-sdk [measured via the GitHub releases API and crates.io]. A 3.x migration guide exists in the repo README.
- **Spec support.** The README says it "implements the stable MCP 2026-07-28 specification while remaining fully compatible with 2025-11-25 and earlier". https://github.com/modelcontextprotocol/rust-sdk/blob/main/README.md [read]
- **Server and client.** Cargo features `server`, `client`, `transport-streamable-http-server`, `transport-streamable-http-client-reqwest`, `transport-child-process` and `auth` exist [measured]. `StreamableHttpService` is a Tower service you mount on axum.
- **Stateless HTTP.** A default server already serves 2026-07-28 clients statelessly. `with_legacy_session_mode(false)` makes legacy versions stateless too. The service factory then runs per request, so shared state must live in a cloneable handle.
- **list_changed.** `peer.notify_tool_list_changed()` covers the legacy session. For the modern path, the `ServerHandler::listen` + `accepted_subscription_filter` + `SubscriptionContext::sink()` pattern is documented in the README.
- **Dynamic registration.** `ToolRouter::add_route`, `remove_route` and `disable_route` exist at tag `rmcp-v3.5.1`. https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/handler/server/router/tool.rs [read]
- **Request identity.** The Tower service injects `http::request::Parts` into request extensions, so a tool handler can read `Authorization` or axum middleware extensions. https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/transport/streamable_http_server/tower.rs [read]
- **Client lifecycle.** `ClientLifecycleMode::{Initialize, Discover, Auto}` exists. `Auto` probes `server/discover` and falls back after a 10 s timeout. [read, README]

### 2.2 Existing aggregators and gateways, and how they namespace tools

| Project | What it is | Namespacing | Source |
|---|---|---|---|
| McpMux | Rust + Tauri 2 desktop gateway. One URL (`http://localhost:45818/mcp`) for many clients. Routes by the client's MCP root folder into Spaces and FeatureSets. OS keychain. GPL-3.0, 24 stars. | Not documented in the README. unverified | https://github.com/mcpmux/mcp-mux [read, measured] |
| MetaMCP | Namespace of servers behind one endpoint, with middleware. | `{ServerName}__{originalToolName}`. Chained gateways nest, for example `Outer__Inner__tool`. Per-namespace overrides of name, title and description. | https://docs.metamcp.com/en/concepts/namespaces [read] |
| LiteLLM | MCP gateway. | Prefixes each tool with its MCP server name. New server names must satisfy SEP-986. | https://docs.litellm.ai/docs/mcp [read] |
| hyper-mcp | Rust MCP server whose tools come from WASM plugins on Extism. stdio only. Plugins from OCI, http(s), s3 or file. OCI plugins are verified with cosign (Sigstore) by default. Per-plugin `allowed_hosts` and `memory_limit`. | "Tool name prefix to prevent tool names collision". The prefix config key was not found. unverified | https://github.com/hyper-mcp-rs/hyper-mcp [read] |
| jilebi | rmcp-based MCP server with JavaScript plugins. TOML manifest per plugin declaring tools, resources, prompts and permissions (`hosts`, `urls`, `read_files`, `write_files`, `read_dirs`, `write_dirs`). 4 stars, AGPL-3.0, so a precedent rather than a dependency. | not documented. unverified | https://github.com/datron/jilebi [read, measured] |
| agentgateway | Rust proxy for MCP, A2A and LLMs. Apache-2.0, about 5.2k stars. | not checked | https://github.com/agentgateway/agentgateway [measured] |

- The other projects I fetched, `mcp-proxy`, Docker `mcp-gateway` and IBM ContextForge, gave no verifiable namespacing facts.
- Claude Code's own convention is `mcp__<server>__<tool>`. Cursor shows a user-visible failure with double-underscore names, but that is a Cursor bug, not a spec rule. https://forum.cursor.com/t/model-fails-to-find-mcp-server-tool-when-name-contains-double-underscores/126744 [search result only, not fetched]

---

## 3. Plugin runtime options

### 3a. WebAssembly Component Model on Wasmtime

- **Versions** [measured via crates.io and the GitHub API]:
  - Wasmtime stable **49.0.2** (2026-10-02).
  - 50.0.0-rc.1 (2026-10-05).
  - **48.0.0** released 2026-08-20.
- **Support policy.** A new major ships monthly on the 20th. Multiples of 12 are LTS with 24 months of support. Others get 2 months. Security fixes are backported to every supported release. https://docs.wasmtime.dev/stability-release.html [read]. By that rule v48 is LTS; the end date is [inferred] about Aug 2028.
- **WASI.** https://wasi.dev/roadmap [read]:
  - WASI 0.3.0 shipped 2026-06-11. 0.3.1 shipped 2026-08-11. 0.3.2 is planned 2026-10-13.
  - Wasmtime 46+ enables 0.3 by default.
  - 0.3 adds native async: `async func`, `stream<T>`, `future<T>`.
- **Toolchain maturity** https://wasi.dev/languages [read]:
  - Rust `wasm32-wasip2` is Tier 2 on stable. `wasm32-wasip3` is Tier 3 and nightly-only.
  - `jco` is JS/TS with 0.2 stable and 0.3 experimental.
  - `componentize-py`, TinyGo and C# (pre-release) produce components at WASI 0.2.
  - Zig has no component toolchain. Swift and Java are planned.
  - "Broad 0.3 support across languages is still landing."
- **`wasmtime-wasi-http` 49.0.2** has a `p2` module, and its `p3` module is "Experimental, unstable and incomplete". It offers a `WasiHttpHooks` trait so the embedder can intercept outbound requests. https://docs.rs/wasmtime-wasi-http/49.0.2/wasmtime_wasi_http/ [read]
- **Sandbox controls** [read]:
  - Fuel (deterministic) or epoch interruption, with trap or async-yield on interrupt. https://docs.wasmtime.dev/examples-interrupting-wasm.html
  - `ResourceLimiter` limits memory, with CPU limited via fuel or epochs. https://docs.rs/wasmtime/49.0.2/wasmtime/trait.ResourceLimiter.html
  - The docs state the goal of "execute untrusted code in a safe manner inside of a sandbox". https://docs.wasmtime.dev/security.html
- **Zed's model** (the closest shipped analogue) [read]:
  - Extensions are Git repos with `extension.toml`, compiled to `wasm32-wasip2` with the `zed_extension_api` crate. Latest on crates.io is 0.7.0 (2025-09-12) [measured]. The repo is at 0.8.0, marked unpublished.
  - The WIT world (`since_v0.8.0/extension.wit`) imports `context-server`, `dap`, `github`, `http-client`, `platform`, `process`, `nodejs`.
  - It exports language-server hooks, `run-slash-command`, `context-server-command`, docs indexing and DAP hooks.
  - **No panel or view interface exists.** Features are languages, debuggers, themes, icon themes, snippets and MCP servers. That absence is [inferred] from the feature list and the WIT.
  - The host keeps bindings for every API version: `wasm_api_version_range` spans `since_v0_0_1::MIN_VERSION..=max_version`, and Stable/Preview are capped lower than Dev/Nightly. https://github.com/zed-industries/zed/blob/main/crates/extension_host/src/wasm_host/wit.rs
  - Capabilities `process:exec`, `download_file` and `npm:install` are user-restrictable through the `granted_extension_capabilities` setting. https://github.com/zed-industries/zed/blob/main/docs/src/extensions/capabilities.md
  - Zed plans to deprecate its MCP-server extensions in favor of the official MCP registry. https://github.com/zed-industries/zed/blob/main/docs/src/extensions/mcp-extensions.md

### 3b. Extism

- **Status.** The crate is 1.30.0 (2026-06-04), BSD-3-Clause, about 5.8k stars. [measured] https://github.com/extism/extism
- **SDKs and PDKs.** There are about 15 host SDKs and 10 PDKs: Rust, JS, Python, Go, Haskell, AssemblyScript, .NET, C, C++, Zig. [read, README]
- **Sandbox config.** The manifest has `allowed_hosts` (empty means none), `allowed_paths` (empty means no access), `memory.max_pages`, `max_http_response_bytes`, `max_var_bytes`, and a wasm `hash`. https://extism.org/docs/concepts/manifest [read]
- **Weaknesses.**
  - It is core-module based. Issue #666, "runtime: wasi preview2 (without Component Model)", has been open since 2024-01. https://github.com/extism/extism/issues/666 [measured]
  - Extism 1.30.0 depends on `wasmtime ^43` and `wasi-common ^43`. v43.0.0 was released 2026-03-20 and is non-LTS, so under the Wasmtime policy it is out of support. Extism `main` already uses wasmtime 48 but is unreleased. [measured]
  - `@extism/js-pdk` on npm is stale (1.1.1, 2024-09-09). The GitHub repo is current (v1.7.0, 2026-08-17), so the npm package is not the source of truth.
- **Existing precedent.** hyper-mcp ships MCP tools from Extism plugins with OCI and Sigstore (section 2.2).

### 3c. Out-of-process plugins (subprocess, LSP or VS Code extension-host style)

- **VS Code** runs extensions in a separate extension host. It prevents extensions from impacting startup, slowing UI operations, and modifying the UI directly. https://code.visualstudio.com/api/advanced-topics/extension-host [read]
  - The extension host "has the same permissions as VS Code itself". https://code.visualstudio.com/docs/editor/extension-runtime-security [read]
  - A process boundary is crash isolation, not a security sandbox.
- **Raycast** runs a single managed Node child process with one V8 isolate per extension. Extensions are "not further sandboxed" for file I/O or network. The RPC exposes a defined set of APIs. https://developers.raycast.com/information/security [read]
- **Claude Code plugins** state the same: a plugin "can execute arbitrary code on your machine with your user privileges", and stdio MCP servers run outside Claude's sandbox. https://code.claude.com/docs/en/plugins/security [read]
- **Enforcing limits on a subprocess** needs OS sandboxing.
  - Claude Code uses `@anthropic-ai/sandbox-runtime`: Seatbelt on macOS, `bubblewrap` and `socat` on Linux and WSL2, plus a filtering network proxy.
  - Its README calls it a "Beta Research Preview" that can sandbox "local MCP servers ... and arbitrary processes". https://github.com/anthropics/sandbox-runtime [read]
  - Native Windows support was not found in the pages I read. unverified.
- **Deno as subprocess runner.** Deno gives per-process `--allow-*` permissions. Its docs still say to layer defenses (limited permissions, a Worker with reduced permissions, OS sandboxing, VMs) for truly untrusted code. https://docs.deno.com/runtime/fundamentals/security/ [read]

### 3d. JS/TS plugins with UI in sandboxed iframes

- **Figma (logic sandbox plus iframe UI)** [read]:
  - Plugin logic runs in "a minimal JavaScript environment" that does not expose browser APIs. UI runs in an `<iframe>` opened with `figma.showUI()`. The two sides talk by message passing. https://developers.figma.com/docs/plugins/how-plugins-run/
  - The manifest declares `networkAccess.allowedDomains`, `permissions`, `api` (version), `editorType`, `documentAccess`. https://developers.figma.com/docs/plugins/manifest/
  - Figma's blog says that after a privately disclosed vulnerability in the Realms shim it moved to "compiling a JavaScript VM written in C to WebAssembly". The post also describes null-origin iframes plus whitelisted messages. https://www.figma.com/blog/how-we-built-the-figma-plugin-system/
- **VS Code webviews** are "an iframe within VS Code".
  - Scripts are off by default (`enableScripts`) and `localResourceRoots` restricts local loads.
  - It recommends a CSP meta tag with `default-src 'none'`.
  - The webview talks to the extension through `postMessage` and `acquireVsCodeApi()`. https://code.visualstudio.com/api/extension-guides/webview [read]
- **Obsidian** plugins are unsandboxed. The docs say "Obsidian cannot reliably restrict plugins to specific permissions or access levels", and offer a global Restricted Mode toggle instead. https://help.obsidian.md/plugin-security (source: obsidian-help repo, `Plugin security.md`) [read]
- **Raycast** extensions are React plus Node. The host renders native UI from the React tree, so there is no iframe. Raycast also has AI-extension "tools" derived from TypeScript input types and JSDoc. https://developers.raycast.com/ai/learn-core-concepts-of-ai-extensions [read]
- **VS Code AI tools** are the closest analogue to plugin-contributed agent tools.
  - Declarative `contributes.languageModelTools` carries `modelDescription`, `userDescription`, `inputSchema` and `toolReferenceName`.
  - The activation event `onLanguageModelTool:<name>` loads the extension lazily.
  - A generic confirmation dialog is always shown for extension tools. https://code.visualstudio.com/api/extension-guides/ai/tools [read]
- **Tauri's plugin system is compile-time Rust.** A plugin is "a Cargo crate and an optional NPM package". `Builder::plugin<P: Plugin<R> + 'static>(plugin)` takes a Rust value. https://v2.tauri.app/develop/plugins/ and https://docs.rs/tauri/2.12.1/tauri/struct.Builder.html [read]. I found no runtime loader for third-party plugins; that is an absence in the docs, not an explicit prohibition.
- **Tauri versions.** Stable is 2.12.1 [measured]. `3.0.0-alpha.4` appeared 2026-10-01 and the release list also contains a `tauri-runtime-cef` tag. I did not evaluate Tauri 3.
- **Tauri iframe caveat.** The capabilities docs warn that on Linux and Android "Tauri is unable to distinguish between requests from an embedded `<iframe>` and the window itself". https://v2.tauri.app/security/capabilities/ [read]

### 3e. Embedded JS engines in the Rust host

- **`rquickjs` 0.14.0** (2026-09-18) [measured]:
  - It is a binding to QuickJS-NG, about ES2020, and "doesn't aim to provide system and web APIs".
  - `Runtime` exposes `set_memory_limit`, `set_interrupt_handler` and `set_max_stack_size`.
  - Cold lifecycle is under 300 microseconds, per the QuickJS claims in the README.
  - https://github.com/DelSkayn/rquickjs and https://docs.rs/rquickjs/0.14.0/rquickjs/struct.Runtime.html [read]
- **`deno_core` 0.412.0** (2026-09-16). The repo was merged into denoland/deno [read]. `deno_runtime` 0.267.0 says its API is "subject to rapid and breaking changes". `deno_permissions` 0.118.0 exists [measured]. https://github.com/denoland/deno/blob/main/runtime/README.md [read]
- **Deno's permission model** is deny-by-default for I/O, but all code on one thread shares one privilege level. https://docs.deno.com/runtime/fundamentals/security/ [read]
- **JS-in-WASM alternatives:**
  - **Javy** (Bytecode Alliance, Apache-2.0, v9.1.0 on 2026-07-30, actively pushed). Static linking gives modules of at least 869 KB, dynamic linking 1-16 KB. https://github.com/bytecodealliance/javy [read]
  - **ComponentizeJS** (SpiderMonkey/StarlingMonkey, WIT-typed). Its README says "experimental project, no guarantees ... breaking changes may be made", with about 8 MB embedding per component. The `jco` npm package is 1.37.0 (2026-10-06) and `componentize-js` 0.23.0 (2026-09-21) [measured]. https://github.com/bytecodealliance/ComponentizeJS [read]
  - **Extism JS PDK**, as in section 3b.

### Trade-off summary

| Option | Sandbox strength | Language reach | UI story | DX | Main cost |
|---|---|---|---|---|---|
| Wasmtime component + own WIT | Strong; same on all OSes | Rust, JS, Python, Go, C# | None built in | Moderate; toolchains vary | WASI 0.3 and toolchain churn, monthly majors |
| Extism | Strong | 10 PDK languages | None | Good | Pins unsupported Wasmtime, core modules only |
| Subprocess plugins | Crash isolation only unless OS-sandboxed | Any | None built in | Best | Install means native code execution; Windows sandbox unknown |
| In-host QuickJS/Deno | Engine bugs hit the host | JS/TS only | None built in | Good | Figma's Realms lesson; Deno API churn |
| JS engine inside WASM | Strong | JS/TS | None built in | Good for JS | Javy size or ComponentizeJS experimental status |

---

## 4. Security

- **Capability manifests** (precedents) [read]:
  - Chrome MV3 splits `permissions`, `host_permissions` and `optional_permissions` / `optional_host_permissions` (granted at runtime). https://developer.chrome.com/docs/extensions/develop/concepts/declare-permissions
  - Zed has user-restrictable host-side capability grants (section 3a).
  - Figma has `networkAccess.allowedDomains`.
  - jilebi and hyper-mcp use per-plugin host, file and directory allowlists (section 2.2).
  - Tauri capabilities are JSON/TOML files in `src-tauri/capabilities` mapping permissions to **window or webview labels**. By default every registered command is callable from every window unless restricted with `AppManifest::commands`. Capability files are compile-time. `Manager::add_capability` exists but only behind the `dynamic-acl` feature. https://v2.tauri.app/security/capabilities/ and https://docs.rs/tauri/2.12.1/tauri/trait.Manager.html [read]
- **Workspace trust.** VS Code lets an extension declare `capabilities.untrustedWorkspaces: { supported: true | false | 'limited' }`. https://code.visualstudio.com/api/extension-guides/workspace-trust [read]. Claude Code also gates project `.mcp.json` approvals behind its trust dialog.
- **CSP for plugin iframes.**
  - The MCP Apps default CSP is in section 1.4.
  - Chrome's extension-page default is `script-src 'self'; object-src 'self';`. https://developer.chrome.com/docs/extensions/reference/manifest/content-security-policy [read]
  - Tauri applies CSP only if you configure it, and appends nonces and hashes for bundled code. https://v2.tauri.app/security/csp/ [read]
  - MDN says that combining `allow-scripts` with `allow-same-origin` on a same-origin embedded document lets it remove its own sandbox. https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/iframe [read]
- **postMessage RPC.** MDN says to always pass an exact `targetOrigin` instead of `*`, and to verify `event.origin` and `event.source` on receive. https://developer.mozilla.org/en-US/docs/Web/API/Window/postMessage [read]. MCP Apps standardizes JSON-RPC 2.0 over postMessage, with a typed SDK.
- **Tauri's Isolation pattern** encrypts IPC through a sandboxed iframe. It addresses supply-chain threats to the app's own frontend, not third-party plugins. https://v2.tauri.app/concept/inter-process-communication/isolation/ [read]
- **Package signing and verification** [read unless noted]:
  - hyper-mcp verifies OCI plugin signatures with cosign (Sigstore) unless `insecure_skip_signature` is set.
  - VS Code's Marketplace signs every extension, and VS Code checks the signature at install. It also runs malware and secret scans and has a verified-publisher badge. https://code.visualstudio.com/docs/editor/extension-runtime-security
  - Obsidian scans every plugin version automatically and shows a scorecard.
  - Raycast requires open-source review plus CI validation. It notarizes and signs its own app.
  - Tauri's updater requires a signature that "cannot be disabled". https://v2.tauri.app/plugin/updater/
  - Claude Code marketplace sources support `sha` (a 40-character commit) and `archive` with `sha256`, and npm sources are fetched "without running install scripts". https://code.claude.com/docs/en/plugins/marketplace-reference
  - Rust crates [measured]: `sigstore` 0.14.0 is described as "experimental"; `minisign-verify` 0.3.0 verifies Ed25519 minisign signatures.
- **Tool-description risk.** The MCP spec says clients MUST treat tool annotations as untrusted unless the server is trusted. https://modelcontextprotocol.io/specification/2026-07-28/server/tools [read]. Claude Code notes plugin skills, commands and agents enter Claude's context as instructions. https://code.claude.com/docs/en/plugins/security [read]. Plugin-supplied tool descriptions reaching the model is a prompt-injection surface. That conclusion is [inferred].

---

## 5. Distribution, versioning and API compatibility

| App | Distribution | Versioning and compatibility | Source |
|---|---|---|---|
| Zed | PR to the `zed-industries/extensions` repo adding a git submodule plus an `extensions.toml` entry. HTTPS submodule URL required. Accepted licenses only. | `schema_version` in `extension.toml`. The host keeps multiple WIT API versions side by side. | https://github.com/zed-industries/zed/blob/main/docs/src/extensions/publishing/publishing-guide.md [read] |
| Obsidian | Submit to the Community directory. A GitHub release per version carrying `main.js`, `manifest.json` and similar. `community-plugins.json` in obsidian-releases has 8,557 entries. | `manifest.json` with `version` as strict `x.y.z` semver and required `minAppVersion`. The release tag must equal the manifest version. | https://docs.obsidian.md/Plugins/Releasing/Submit+your+plugin and https://docs.obsidian.md/Reference/Manifest [read, measured] |
| Raycast | `npm run publish` opens a PR to `raycast/extensions`. Open source, reviewed, then CI-validated. | No version in the manifest. The store has one implicit latest. Backward-compatible within a major API. The host checks compatibility between the used `@raycast/api` and the app. | https://developers.raycast.com/basics/publish-an-extension and https://developers.raycast.com/information/versioning [read] |
| VS Code | Marketplace via `vsce`, plus Open VSX. | `engines.vscode` is required (cannot be `*`). `contributes` and `activationEvents` enable declarative, lazy loading. | https://code.visualstudio.com/api/references/extension-manifest [read] |
| Figma | Review and publish through Figma. | `api` version in the manifest. | https://developers.figma.com/docs/plugins/manifest/ [read] |
| Claude Code | `marketplace.json`. Plugin sources `github`, `url`, `git-subdir`, `npm`, `archive`, with `ref` and `sha` pinning. Dependencies take semver ranges (`^2.0`, `~2.1.0`). | `plugin.json` `version`. Unconstrained dependencies move to each new release. | https://code.claude.com/docs/en/plugins/marketplace-reference and https://code.claude.com/docs/en/plugins/dependencies [read] |
| MCP itself | Official registry at registry.modelcontextprotocol.io, release v1.8.1 (2026-08-06). | Deprecation policy: at least 12 months between Deprecated and removal. | https://github.com/modelcontextprotocol/registry [read] and https://modelcontextprotocol.io/specification/2026-07-28/deprecated |

---

## 6. Recommendation

The manifest shape, naming scheme, and event schema below are my design proposal, built from the cited precedents. They are not verified facts.

### Architecture in one paragraph

A plugin is a signed package containing a declarative manifest, optional sandboxed logic, and optional UI. Wings core is a Rust (Tauri 2) process that owns five things:

- a plugin registry;
- a **Wasmtime component host** for logic;
- **sandboxed iframes** for UI, speaking JSON-RPC over postMessage in the MCP Apps dialect;
- a **session event bus** fed by Claude Code hooks;
- a **single rmcp MCP gateway** that exposes plugin tools to Claude Code.

### Manifest (`wings-plugin.toml`)

```toml
[plugin]
id = "git-insights"                # [a-z0-9-]+, no underscores
name = "Git Insights"
version = "1.2.0"                  # semver
api = "1"                          # major of the wings:plugin WIT world
license = "MIT"
publisher = "acme"

[logic]                            # optional
kind = "wasm-component"            # Tier 1 default; "sidecar" is Tier 2
file = "plugin.wasm"
limits = { memory_mb = 64, cpu_ms_per_call = 200 }

[permissions]                      # deny by default, shown at install
events = ["session.prompt", "session.tool_use", "session.turn_end"]
transcript = "none"                # none | summary | read
net = ["api.github.com"]
fs_read = ["${workspace}/.git"]
storage = "kv:1MB"
optional = ["net:*.example.com"]   # granted at runtime, Chrome-style

[[tools]]
name = "blame_range"               # snake_case; exposed as mcp__wings__git-insights_blame_range
description = "..."
input_schema = "schemas/blame.json"
confirm = "first-use"              # never | first-use | always (host enforces)

[[views]]
id = "panel"
entry = "ui/index.html"
slot = "sidebar"
subscribe = ["session.*"]
csp = { connect = [] }

[[commands]]  # slash commands and palette entries, surfaced as MCP prompts where useful
```

### Where logic runs

- **Tier 1 (default): WASM component** in the Wings core under Wasmtime. A plugin is a component that exports `init`, `call-tool`, `on-event` and `on-timer` from a Wings-owned WIT world `wings:plugin@1`.
  - The host imports only `wings:host/{log, kv, http, session, fs-scoped}`, each gated by the manifest.
  - Define Wings' own `http` interface rather than depending on `wasi:http`, because the p3 version is still experimental.
  - Apply fuel or epoch limits plus `ResourceLimiter` per instance.
  - Activate lazily on the first matching tool call or event, as VS Code's `onLanguageModelTool` does.
  - Pin **Wasmtime 48 (LTS)**.
- **TS authoring.** Ship a TS SDK and a build command that bundles TS and links it into a JS-engine-in-WASM. Javy (QuickJS) is the smaller, production-ish engine. ComponentizeJS is WIT-native but experimental and heavy. Pick one, hidden behind the SDK. Rust authors use `wit-bindgen` directly.
- **Tier 2 (explicit high-trust, later): subprocess sidecar** speaking MCP or JSON-RPC over stdio.
  - Use it for plugins that must spawn tools or load native libraries.
  - Require install-time consent and verified-publisher status. Wrap with `sandbox-runtime` where it is available.
- **Tier 0: declarative-only plugins** (views, prompts, static resources) carry no code risk.

### How UI is hosted

- Serve plugin UI bundles from a Tauri custom URI scheme (`register_uri_scheme_protocol`, verified to exist), one origin per plugin. Render them in `<iframe sandbox="allow-scripts">`, or in a child webview (the multiwebview API needs Tauri's `unstable` feature, desktop only).
- Follow the MCP Apps sandbox-proxy model so the host and sandbox origins differ.
- Use `@modelcontextprotocol/ext-apps/app-bridge` (or `@mcp-ui/client`) on the Wings side. Plugin authors get the standard `App` class and React hooks, and views can also work in Claude Desktop.
- Extend the dialect with `wings/*` notifications for session events. Generate TS types for the Wings extensions from the same Rust types and JSON Schema (`schemars` 1.2.2, `ts-rs` 12.0.1, `specta` 1.0.5, all measured).
- Keep plugin UI out of the privileged Tauri IPC path. Restrict commands with `AppManifest::commands`, give plugin webviews no capability, and treat `postMessage` as the only channel. I have not tested whether an iframe on Linux can reach `ipc://`. unverified.

### How tools reach Claude Code (single MCP server)

- **Registration.** Wings launches `claude` itself with `--mcp-config <0600 file>` containing `{"mcpServers":{"wings":{"type":"http","url":"http://127.0.0.1:<port>/mcp","headers":{"Authorization":"Bearer <per-session-token>"}}}}`.
  - Put the token in a file rather than argv.
  - Do not use `--strict-mcp-config`, which would drop the user's other servers.
  - Do not use project `.mcp.json` (approval prompt, pollutes repos) or a Claude plugin (the tool names become `mcp__plugin_...`).
  - Sessions the user starts by typing `claude` manually would need a PATH shim. [inferred]
- **Tool names.** Expose `{plugin_id}_{tool}`. Plugin ids have no underscore and tool names use underscores only, so the first underscore splits unambiguously. Avoid dots, because the Claude API regex forbids them. Claude then shows `mcp__wings__git-insights_blame_range`, which stays well under 128 characters.
- **Per-session tool sets.** Resolve the bearer token to a session and its enabled plugins in rmcp middleware via `http::request::Parts`. The 2026-07-28 spec permits list variance by authorization. In the legacy era the session does the same job.
- **Dual-era server.** rmcp 3.5.1 handles both.
  - Notify through the legacy peer (`notify_tool_list_changed`) and through the `listen` sink.
  - Implement `ServerHandler::list_tools` and `call_tool` over a plugin registry. `ToolRouter::add_route` exists, but a registry is simpler for dynamic plugin tools. [inferred]
- **Content.** Write good server `instructions` and keep tool descriptions short (Claude Code truncates at 2,048 characters and defers tools behind tool search). The host, not the plugin, sets safety annotations unless the user approves them.
- **Long-running tools.** Tools that wait on Wings UI approval must send progress notifications within 5 minutes (HTTP idle window) and stay under the per-server `timeout`.

### How plugins subscribe to session and transcript events

- **Ingest from Claude Code hooks.** Launch `claude` with `--settings '{"hooks": {...HTTP hooks to http://127.0.0.1:<port>/hooks/<session-token>...}}'`. The documented events include `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `SubagentStop`, `Notification`, `PreCompact`, `SessionEnd` and `MessageDisplay` (streaming `delta`). Hook entries merge across settings levels. https://code.claude.com/docs/en/hooks [read]
  - Respond to non-blocking events immediately.
  - Org policy `allowedHttpHookUrls` can block HTTP hooks. [read]
- **Do not make the transcript the primary source.** The docs say the JSONL entry format "is internal to Claude Code and changes between versions". https://code.claude.com/docs/en/sessions#where-transcripts-are-stored [read]. Use `transcript_path` only as backfill behind an adapter.
- **Normalize into a Wings-owned versioned schema** (for example `wings:session/event`). Plugins never see Claude Code's raw formats, so Claude Code changes break one adapter, not every plugin.
- **Delivery.** Permission-gated events go to the WASM `on-event` export and to subscribed UI iframes through host-pushed notifications. The host filters by manifest `events` and `transcript` permissions. Plugins never get file access.
- Channels are a research preview with an allowlist. The raw PTY stream is unstructured. Neither is a primary path.

### Packaging, versioning, trust

- **Package** as an archive with the manifest, `plugin.wasm`, `ui/`, schemas, and a SHA256 list.
- **Registry** as a Git-hosted index reviewed by PR, in the Zed and Obsidian style. Pin `sha256` plus `ref` and `sha`, as Claude Code's marketplace does.
- **Signing.** Phase 1 is a publisher Ed25519 minisign signature, verified with `minisign-verify`. Phase 2 is Sigstore keyless provenance, shelling out to `cosign` as hyper-mcp does, since the Rust `sigstore` crate is experimental.
- **Compatibility.** Manifest `api = "1"`. The host supports N and N-1 of the WIT world (Zed keeps every version). Use a 12-month deprecation window as in MCP's policy.
- **Description pinning.** Pin a hash of each tool description in a lockfile and re-prompt on change, so a plugin cannot alter what the model reads after approval. [inferred risk mitigation]
- **Trust.** Per-workspace plugin enablement, gated like VS Code Workspace Trust.

### Phasing

1. Manifest, UI iframes, event bus, gateway, declarative tools. No plugin code yet.
2. WASM logic with the Rust SDK, then the TS SDK.
3. Registry and signing.
4. Optional sidecar tier.

### Nearest rival and why it loses

- **Overall rival: plugins as separate MCP servers behind an aggregator** (McpMux, MetaMCP, hyper-mcp style). It is the least new code, and it gives the widest language reach and best DX.
  - It loses because installing a plugin means running native code as the user. The documented examples (Claude plugins, Raycast, Obsidian, the VS Code extension host) say so.
  - Parity of OS sandboxing across macOS, Linux and Windows is unverified.
  - MCP has no standard session-event push, so event delivery would be invented anyway.
  - UI still needs a separate channel.
  - It survives as the opt-in Tier 2.
- **Logic runtime rival: in-process `rquickjs`.** It loses on Figma's documented move off an in-JS sandbox. It also puts a C engine in the host address space and gives no capability boundary. [Figma blog read; engine-in-process risk inferred]
- **Host rival: Extism.** It loses on the pinned unsupported Wasmtime and the missing Component Model. It is acceptable if you accept those. Its manifest ideas are worth borrowing.
- **UI rival: Raycast-style host-rendered React tree.** It loses because it requires Wings to build a component library and a reconciler, and it cannot embed rich session-reactive widgets such as diffs or charts. It also does not reuse the MCP Apps ecosystem.
- **Identity rival: MCP `roots` (McpMux's folder-routing) or `Mcp-Session-Id`.** Roots are deprecated, and `Mcp-Session-Id` is gone in 2026-07-28. A per-session bearer token works in both eras.
- **Transport rival: a stdio bridge process per session.** It has a longer idle window (30 minutes versus 5) and no TCP port. It loses because Claude Code does not auto-reconnect stdio servers, and it needs an extra process plus IPC to the app.

---

## Unverified or open (not settled in this research)

- Whether Claude Code Desktop's Code tab renders MCP Apps, and whether the CLI advertises `io.modelcontextprotocol/ui`. The docs I fetched are silent.
- How Claude Code handles sampling, the Tasks extension and `structuredContent`.
- Whether `--mcp-config` entries are exempt from approval prompts. The docs list `.mcp.json` as the prompted source, not `--mcp-config`.
- Whether `--settings` hook entries merge with user hooks. The docs say hook entries merge across settings levels, and `--settings` is one level, so I infer it does. I have not run it.
- How tool-name prefixing is configured in hyper-mcp and McpMux. No documentation found.
- Per-platform custom-scheme origin form and CSP header support in Tauri webviews (WebKit, WebView2, WebKitGTK). Needs a cross-platform test.
- Whether an iframe can reach Tauri IPC on Linux when it sits inside a capability-bearing webview.
- Native Windows support in `sandbox-runtime`.
- Claude Code's undocumented IDE lockfile integration, which I did not investigate.
- I did not evaluate Tauri 3.0.0-alpha or its CEF runtime.

## Raw sources on disk

The raw sources are in `/private/tmp/...scratchpad/plugin-research/`:

- `spec2026/` and `spec2025/`: MCP spec pages
- `apps/`: MCP Apps, extension matrix, ext-apps README and spec
- `claude/`: Claude Code docs (mcp, hooks, cli-reference, plugin docs, sandboxing, and so on)
- `gh/`: rust-sdk README and tool router source, gateway READMEs
- `wasi/`, `zed/`, `extism/`, `tauri/`, `vscode/`, `figma/`, `obsidian/`, `raycast/`, `chrome/`, `js/`