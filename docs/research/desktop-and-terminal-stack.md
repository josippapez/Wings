> Research notes gathered 2026-10-08 by a research agent. Paths under `/private/tmp/...scratchpad/` were temporary and no longer exist. Treat versions and issue states as of that date.

# Wings stack research: desktop UI framework and terminal emulator

Date of research: 2026-10-08. No benchmarks were run. Every fact below was fetched live this session (crates.io / npm APIs, GitHub API and raw files, official docs). Basis tags: [read] = I read the source or docs, [others] = a third party's measurement or report, [self] = project self-reported, [inferred] = my reasoning, labelled as such. Downloaded material is only read (never executed) under `/private/tmp/...scratchpad/stack-research/` (subdirs: crates, npm, gh, tauri-docs, tauri-src, tauri-cef, webkitgtk, xterm, pty, ghostty, gpui, apps, claude-docs). Helper script: `.../scratchpad/tools/issue.sh`.

---

## A. UI frameworks

### A.1 Tauri 2.x

**Version and maturity**
- Tauri 2.12.1 was published 2026-09-30 (https://github.com/tauri-apps/tauri/releases). The 2.0.0 stable release was 2024-10-02 and 2.12.0 was 2026-09-26 (https://crates.io/api/v1/crates/tauri/versions).
- The 2.12 announcement calls it "the biggest update so far in the 2.x releases". It drops official Windows 7 support and sets MSRV to 1.90 (https://v2.tauri.app/blog/tauri-2.12/).
- Tauri 3 is alpha only. `tauri` 3.0.0-alpha.4 is dated 2026-10-01 and alpha.0 was 2026-09-13 (https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.0). Do not build on v3 yet.
- The repo has 111,668 stars, was pushed 2026-10-08 and is Apache-2.0 (https://github.com/tauri-apps/tauri). Related crates: wry 0.57.0 (2026-09-08), tao 0.37.1 (2026-09-26), tauri-plugin-updater 2.13.2 (2026-10-07) (https://crates.io/crates/wry, https://crates.io/crates/tao).

**Rendering**
- Tauri uses the OS webview: WKWebView on macOS, WebView2 (Chromium) on Windows, WebKitGTK on Linux (https://v2.tauri.app/reference/webview-versions/ and the section "Webview Versions" in https://v2.tauri.app/llms-full.txt).
- Tauri 2 needs the WebKitGTK 4.1 API (`libwebkit2gtk-4.1-dev`) (https://github.com/tauri-apps/wry README).

**IPC** [read]
- Commands are JSON-RPC-like, so arguments and results must be JSON. Events are fire-and-forget. The docs say events are "not designed for low latency or high throughput", that payloads are always JSON strings, and that Channels are the optimized streaming path (https://v2.tauri.app/develop/calling-frontend/).
- Returning `tauri::ipc::Response` sends an ArrayBuffer without JSON (https://v2.tauri.app/develop/calling-rust/). A command can also read the raw request body as `tauri::ipc::Request`.
- Channel facts from source at tag `tauri-v2.12.1` (https://github.com/tauri-apps/tauri/blob/tauri-v2.12.1/crates/tauri/src/ipc/channel.rs):
  - A Channel is ordered. Each message carries an index and the JS side buffers out-of-order messages (lines 137-141).
  - For raw bytes, send `InvokeResponseBody::Raw` through `Channel<InvokeResponseBody>`. A `Channel<Vec<u8>>` is NOT raw: it is sent as a JSON array of numbers (lines 143-148).
  - Small payloads (JSON under 8192 bytes, raw under 1024 bytes) are delivered via `webview.eval(...)`. Small raw chunks are even encoded as `new Uint8Array([...json array...])`. Larger payloads go through a `fetch` of the custom-protocol command `plugin:__TAURI_CHANNEL__|fetch` (lines 34-37 and 296-321). Source comments say 8 KB JSON is about 2x faster via eval than fetch on WebView2 v135, and 1 KB is about 30% faster via eval on macOS.
  - Design consequence [inferred]: coalesce PTY output on the Rust side to at least about 1 KB per message, and per frame, rather than sending tiny reads.
  - There is a `Builder::channel_interceptor` hook (app.rs:1763).
- I found no published Channel-vs-events benchmark or PTY-over-Tauri MB/s figure. The only throughput data is a user report in https://github.com/tauri-apps/tauri/issues/13405 ("200ms to send only 3MB" over IPC, 2025-05, image-processing use case, not a benchmark). The same issue notes that event payloads cannot be raw ArrayBuffers (still open).

**Isolation pattern**
- It routes every IPC message through a sandboxed `<iframe>` hook and encrypts it with AES-GCM using a per-run key.
- Docs limitations: ES modules do not load in the isolation app on Windows, and there is some encryption overhead (https://v2.tauri.app/concept/inter-process-communication/isolation/).
- Open bug filed 2026-10-06: on Windows/WebView2 the isolation iframe gets an opaque origin, `crypto.subtle` is undefined and ALL IPC silently hangs (https://github.com/tauri-apps/tauri/issues/16219).

**Capabilities and permissions**
- Capabilities bind permissions to window labels and/or webview labels (glob patterns), optionally to `remote.urls` (URLPattern), and optionally to platforms. For multiwebview windows the docs recommend `webviews` over `windows` for fine-grained control (https://v2.tauri.app/security/capabilities/ and the reference schema in https://v2.tauri.app/llms-full.txt).
- The runtime authority resolves (command, window label, webview label, Origin = Local or Remote{url}) (https://github.com/tauri-apps/tauri/blob/tauri-v2.12.1/crates/tauri/src/ipc/authority.rs).
- The `dynamic-acl` feature is on by default and exposes `Manager::add_capability(impl RuntimeCapability)`, so permissions can be granted at runtime (https://github.com/tauri-apps/tauri/blob/tauri-v2.12.1/crates/tauri/src/lib.rs, around line 840).
- The security docs say capabilities do NOT protect against malicious or lax Rust code, incorrect scope checks, or webview 0-days. Docs caution: "On Linux and Android, Tauri is unable to distinguish between requests from an embedded `<iframe>` and the window itself."

**Sidecars and plugins**
- Sidecars use `bundle.externalBin` with a `-$TARGET_TRIPLE` suffix on the binary name (https://v2.tauri.app/develop/sidecar/).
- GUI apps on macOS and Linux do not inherit shell `$PATH`. Tauri points to the `fix-path-env-rs` crate (https://v2.tauri.app/distribute/appimage/).
- A Tauri plugin is a Cargo crate plus an optional npm package, so it is compile-time (https://v2.tauri.app/develop/plugins/). Tauri has no runtime-loaded third-party plugin mechanism, so Wings must build its own manifest + web bundle layer.
- Custom URI schemes: `register_uri_scheme_protocol` exists. Origins differ by OS: `<scheme>://localhost/` on macOS, iOS and Linux, and `http://<scheme>.localhost/` on Windows and Android (https://github.com/tauri-apps/tauri/blob/tauri-v2.12.1/crates/tauri/src/app.rs, lines 2256-2266).

**Multiwebview / child webviews**
- Still behind the `unstable` Cargo feature in 2.12.1. Its feature list contains `unstable` (https://crates.io/api/v1/crates/tauri/2.12.1). Docs: creating child webviews with `new Webview(...)` needs `unstable`, while `WebviewWindow` is stable (https://v2.tauri.app/llms-full.txt, JS API reference).
- wry says child webviews are supported on macOS, Windows and Linux (X11 only). For X11+Wayland it recommends a `gtk::Fixed` container (https://github.com/tauri-apps/wry README).
- Known issues:
  - macOS: arrow keys in inputs/textareas insert stray control chars (U+001C-1F) on the `unstable` child-webview path. This is open and updated 2026-07-02, and the reporter says saved user data gets corrupted (https://github.com/tauri-apps/tauri/issues/10194). GitButler hits the same issue (comment in that thread).
  - macOS: double characters on the first keystroke (https://github.com/tauri-apps/tauri/issues/8705, open).
  - Linux: child webviews are packed into a GtkBox so `set_bounds` does nothing and webviews stack (https://github.com/tauri-apps/tauri/issues/10420, open; https://github.com/tauri-apps/tauri/issues/16132 closed as a duplicate). Wayland has no positioning at all because wry's bounds handling is X11-only.
  - Fix PRs #15463 and #15704 are still open as of 2026-10-08, and wry #1745 is open (https://github.com/tauri-apps/tauri/pull/15463, https://github.com/tauri-apps/tauri/pull/15704, https://github.com/tauri-apps/wry/pull/1745). A maintainer wrote in 2025-05 that "multiwebview doesn't work on wayland" (https://github.com/tauri-apps/tauri/issues/13405).
- Bottom line [inferred]: do not put terminals, or anything text-input heavy, in `unstable` child webviews. Use one main webview plus iframes, or separate `WebviewWindow`s, for plugin UI.

**Iframes and IPC per platform** [read]
- Tauri's IPC bridge init scripts are registered as `for_main_frame_only: true` (https://github.com/tauri-apps/tauri/blob/tauri-v2.12.1/crates/tauri/src/manager/webview.rs, lines 161-220). `initialization_script_for_all_frames` is the opt-in for iframes.
- wry maps this to `TopFrame` on WebKitGTK and WKWebView. On Windows, "scripts are always added to subframes regardless of the `for_main_frame_only` option" (https://github.com/tauri-apps/wry/blob/dev/src/lib.rs line 1029; the WebView2 implementation just calls `AddScriptToExecuteOnDocumentCreated`).
- So a plugin iframe has no `__TAURI_INTERNALS__` on macOS/Linux, but the bridge object is present in iframes on Windows. Whether the ACL then admits the call depends on the origin check, which I did not trace [unverified]. Design the plugin API as `postMessage` to the host, which proxies after a permission check, and test it on all three OSes.

**Notable apps built on Tauri 2**
- opcode (22,421 stars, AGPL-3.0, https://github.com/winfunc/opcode).
- Jean (1,314 stars, https://github.com/coollabsio/jean).
- TUICommander (160 stars, https://github.com/sstraus/tuicommander).
- Terminal-64 (16 stars, https://github.com/Pugbread/Terminal-64).
- GitButler (uses `unstable` multiwebview, per the #10194 comment).

### A.1b Cross-platform specifics

**(1) Webview engine per OS and known problems**

| OS | Engine | Notes |
|---|---|---|
| macOS | WKWebView | System component, updates with the OS. Unsupported macOS versions get no WebKit updates (https://v2.tauri.app/reference/webview-versions/). |
| Windows | WebView2 (Chromium) | Updatable. `webviewInstallMode` options include `offlineInstaller` (+~127 MB) and `fixedRuntime` (+~180 MB, pins the Chromium version) (https://v2.tauri.app/distribute/windows-installer/). |
| Linux | WebKitGTK 4.1 (via distro packages) | Current upstream is WebKitGTK 2.54.1 (2026-10-02) (https://webkitgtk.org/). |

- WebKitGTK 2.54 adds a new Skia-based compositor and removes the Cairo 2D path (https://webkitgtk.org/2026/09/16/webkitgtk-2.54-highlights.html). 2.52 improved 2D canvas acceleration through batched replay (https://webkitgtk.org/2026/03/18/webkitgtk-2.52-highlights.html). Which version end users actually get depends on their distro, and I did not check distro versions [unverified].
- Tauri's own Linux page lists these symptoms: blank or white window, flicker on resize, death on resize with no error, "AcceleratedSurfaceDMABuf was unable to construct a complete framebuffer", and Wayland "Error 71 (Protocol error)". Most are blamed on the WebKitGTK DMABUF renderer requesting buffer formats NVIDIA does not provide (https://v2.tauri.app/develop/debug/linux-graphics/, last updated 2026-06-15).
- Documented workarounds, in order, where the first two keep hardware acceleration:
  1. `nvidia_drm.modeset=1` (needed for drivers older than 545).
  2. `__NV_DISABLE_EXPLICIT_SYNC=1`.
  3. `WEBKIT_DISABLE_DMABUF_RENDERER=1` (loses the faster path).
  4. `WEBKIT_DISABLE_COMPOSITING_MODE=1` (disables accelerated compositing entirely).
  - Do not set 3 or 4 unconditionally: that page warns it slows healthy machines.
- On WebGL and canvas, the same page says: "WebGL2 context creation succeeds even when the result is backed by a software rasterizer or a slow presentation path". WebKitGTK also masks the renderer string, so `WEBGL_debug_renderer_info` reports "Apple GPU" on every Linux machine. It names "terminal emulators, editors, maps, charts" as the symptom cases and recommends a non-WebGL fallback on Linux plus a user setting.
- The tracking issue is https://github.com/tauri-apps/tauri/issues/9394 (open, updated 2026-01-26). It also reports that WebGL2 stopped working on X11 for some setups, and that Jean applies the DMABUF and compositing flags automatically.
- Open Linux bugs as of 2026-10: NVIDIA window failure (https://github.com/tauri-apps/tauri/issues/9304, updated 2026-09-29), Wayland Error 71 (https://github.com/tauri-apps/tauri/issues/10702, updated 2026-10-07), NVIDIA transparent-window artifacts (https://github.com/tauri-apps/tauri/issues/14924), blank window under Xvfb until compositing is disabled (https://github.com/tauri-apps/tauri/issues/15936).

**Chromium/CEF/Servo backend**
- A pluggable runtime exists in the v3 alpha line. `tauri-runtime-cef` 3.0.0-alpha.5 was published 2026-10-01 (https://crates.io/crates/tauri-runtime-cef), pinned to `cef =152.3.0` (https://github.com/tauri-apps/tauri/blob/v3/crates/tauri-runtime-cef/Cargo.toml).
- In v3 the runtime is chosen at build time: `tauri::Builder::default().runtime(tauri_runtime_wry::Wry::default())` or `.runtime(tauri_runtime_cef::Cef::default())`. The `wry` and `cef` features were removed from `tauri` and `unstable` moves onto the runtime crate (https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.0).
- 2.12.1 has a `wry` feature and no `cef` feature (https://crates.io/api/v1/crates/tauri/2.12.1). CEF is v3 only.
- CEF maturity: alpha. Open CEF bugs include WebGPU only offering SwiftShader on Linux (https://github.com/tauri-apps/tauri/issues/16203), Linux drag-and-drop broken (https://github.com/tauri-apps/tauri/issues/16131), IPC broken when DevTools is open on Linux (https://github.com/tauri-apps/tauri/issues/15764), AppImage bundling failure (https://github.com/tauri-apps/tauri/issues/16204), and a macOS `Menu.new()` deadlock (https://github.com/tauri-apps/tauri/issues/15888). CEF multiwebview on Linux/X11 was fixed in merged PR #15534 (https://github.com/tauri-apps/tauri/pull/15534).
- A shipping app, Koharu 0.83.5, runs on `tauri-runtime-cef` 3.0.0-alpha (named in a comment on #16203).
- Maintainer statements, 2026-02-15, on https://github.com/tauri-apps/tauri/issues/14944: plans for winit plus their own gtk4 backend, and "plans for Servo and a CEF(chromium) based webview". CEF is "mostly developed at the workplace of some of our maintainers... isn't open to outside contributions atm", and QtWebEngine is a "more hopeful" replacement for WebKitGTK, "the part of tauri that needs replacement the most".
- wry's migration to gtk4/webkit6 is an open community PR (https://github.com/tauri-apps/wry/pull/1767).
- Servo: the Verso browser repo is archived ("no longer maintained", last push 2025-10-08, https://github.com/versotile-org/verso). `tauri-runtime-verso` is not archived and was last pushed 2026-09-10 (https://github.com/versotile-org/tauri-runtime-verso). The original blog says it is "not as feature rich" as the production backends (https://v2.tauri.app/blog/tauri-verso-integration/). A maintainer says Servo is "not mature enough for general use today" (issue #14944).
- I found no CEF bundle-size figure [unverified].

**(2) xterm.js WebGL behaviour per webview**
- Hard WebGL context cap: WebKit source has `maxActiveContexts = 16` for the main thread. The 17th context recycles (loses) the oldest (https://github.com/WebKit/WebKit/blob/main/Source/WebCore/html/canvas/WebGLRenderingContextBase.cpp, lines 195 and 336-349).
- Chromium's cap is a preference value (https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/modules/webgl/webgl_rendering_context_base.cc, lines 204 and 437). An xterm.js issue comment asserts about 16 [others, not verified for Chromium].
- xterm issue "Support dozens of terminals on a single page" (open, updated 2026-09-11) is the multiplexer use case. A 2026-09-11 comment from an Electron multi-panel terminal app says the oldest context is silently killed past about 16 (https://github.com/xtermjs/xterm.js/issues/4379). WebglAddon has an `onContextLoss` event (https://github.com/xtermjs/xterm.js/blob/6.0.0/addons/addon-webgl/typings/addon-webgl.d.ts).
- macOS / WKWebView:
  - Partial-row ghosting in Tauri WKWebView on stable macOS during Claude Code streaming. Open, 2026-04-27, updated 2026-08-11 (https://github.com/xtermjs/xterm.js/issues/5847).
  - A fix PR, "atlas page merges" (https://github.com/xtermjs/xterm.js/pull/5883, merged 2026-05-21), was reported on 2026-08-11 as NOT fixing it. That reporter says it also reproduces on Chromium/Windows 11 (Electron 43) with `@xterm/addon-webgl@0.20.0-beta.298` + `@xterm/xterm@6.1.0-beta.302`, i.e. not WKWebView-specific.
  - Safari on macOS 26.5 beta: totally broken WebGL, workaround was the canvas addon (https://github.com/xtermjs/xterm.js/issues/5816, open).
  - The canvas addon is dead: the last `@xterm/addon-canvas` is 0.7.0 (2024-04-05) and the removal issue is closed (https://www.npmjs.com/package/@xterm/addon-canvas, https://github.com/xtermjs/xterm.js/issues/4779). xterm 6 therefore has DOM and WebGL renderers only.
  - Input/IME bugs in WKWebView: dead keys (https://github.com/xtermjs/xterm.js/issues/5894), a white scrollbar gutter flash (https://github.com/xtermjs/xterm.js/issues/6190), dropped IME characters (https://github.com/xtermjs/xterm.js/issues/6144), all open.
- Windows / WebView2 is Chromium, so Chromium behaviour applies. I found no WebView2-specific xterm bug beyond the Chromium/Windows report above.
- Linux / WebKitGTK: no xterm.js issue mentions WebKitGTK. The Tauri docs page above is the evidence, and I found no measured xterm.js frame-rate data on any of the three engines.
- WebGPU: there is an experimental `@xterm/addon-webgpu` PR (https://github.com/xtermjs/xterm.js/pull/6137, open, 2026-08-27). It explicitly claims no performance advantage and is only CI-tested on Chromium. The maintainer closed the WebGPU tracking issue as out of scope on 2025-12-27 (https://github.com/xtermjs/xterm.js/issues/4552). Nothing is published on npm (`@xterm/addon-webgpu` returns 404).
- CEF WebGPU on Linux currently only offers SwiftShader (issue #16203 above).

**(3) PTY on Windows and shell detection**
- `portable-pty` 0.9.0 was published 2025-02-11 with about 10.9M recent downloads (https://crates.io/crates/portable-pty). It uses ConPTY and requires Windows 10 1809+ ("Windows 10 October 2018 or newer"). It first tries a sideloaded `conpty.dll` + `OpenConsole.exe` next to the exe, then falls back to `kernel32`. Flags set: INHERIT_CURSOR | RESIZE_QUIRK | WIN32_INPUT_MODE, and `PSEUDOCONSOLE_PASSTHROUGH_MODE` is defined but unused (https://github.com/wezterm/wezterm/blob/main/pty/src/win/pseudocon.rs).
- Known issues:
  - ConPTY emits `ESC[6n` (cursor-position report) at startup and the terminal side must answer. Unanswered, reads stall. The maintainer replied to the 0.9.0 report that this "sounds like a deadlock because you are not responding" (https://github.com/wezterm/wezterm/issues/6783, open, updated 2026-07-13). xterm.js answers CPR itself, but only if its input path is wired back to the PTY writer [inferred].
  - Dropping the slave handle breaks writes, and reads can hang after the child exits (https://github.com/wezterm/wezterm/issues/4206).
  - An old bundled OpenConsole/conpty pair caused a pwsh FailFast crash. Swapping in the Microsoft ConPTY nupkg fixed it (https://github.com/wezterm/wezterm/issues/7774). The latest Windows Terminal release v1.25.2733.0 (2026-10-02) ships `Microsoft.Windows.Console.ConPTY.1.25.260930003.nupkg` (https://github.com/microsoft/terminal/releases/tag/v1.25.2733.0).
  - A console window flashes in Tauri release builds on Windows (https://github.com/wezterm/wezterm/issues/6946, open).
  - ConPTY passthrough was requested at https://github.com/microsoft/terminal/issues/1173 (closed).
- For xterm.js on a Windows PTY, set `windowsPty: { backend: 'conpty', buildNumber }`. Without it, reflow is disabled and extra rows go into scrollback. `windowsMode` was removed in 6.0 (https://github.com/xtermjs/xterm.js/blob/6.0.0/typings/xterm.d.ts, line 303 and the 6.0.0 release notes https://github.com/xtermjs/xterm.js/releases/tag/6.0.0).
- Shell detection in portable-pty (https://github.com/wezterm/wezterm/blob/main/pty/src/cmdbuilder.rs):
  - Unix: `$SHELL` first, then the passwd entry (checked executable), then `/bin/sh`.
  - Windows: `%ComSpec%`, then `cmd.exe`. It does NOT detect pwsh or WSL. Wings needs its own detection list. Claude Code's docs allow PowerShell, CMD, Bash or WSL on native Windows, and Git for Windows is optional (https://code.claude.com/docs/en/setup).
- `tauri-plugin-pty` 0.3.1 (2026-07-08, about 50k downloads) is a thin wrapper over `portable-pty ^0.9.0` (https://crates.io/crates/tauri-plugin-pty).
- `alacritty_terminal` 0.26.0 (2026-04-06) has its own PTY, including a Windows `escape_args` option (https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/CHANGELOG.md). TUICommander, which spawns many PTYs, uses portable-pty 0.9 + alacritty_terminal 0.26 (https://github.com/sstraus/tuicommander/blob/main/src-tauri/Cargo.toml).

**(4) Packaging, updater, signing** (https://v2.tauri.app/llms-full.txt sections: "AppImage", "macOS Code Signing", "Windows Code Signing", "Linux Code Signing", "Updater")
- Updater plugin: signature verification is mandatory and cannot be disabled. The public key goes in `tauri.conf.json`. The private key goes in `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (`.env` files do not work). Losing the key means installed apps can no longer update. Artifacts (`createUpdaterArtifacts: true`):
  - Linux: the AppImage plus `.sig`. The docs list only AppImage for Linux updates. I did not confirm that deb/rpm cannot update [unverified].
  - macOS: `.app.tar.gz` plus `.sig`.
  - Windows: NSIS `-setup.exe` and MSI plus `.sig`.
  - `createUpdaterArtifacts: "v1Compatible"` is removed in v3 (https://v2.tauri.app/plugin/updater/).
- `tauri-apps/tauri-action` (v1.0.0, 2026-06-29) builds macOS, Linux and Windows and can upload a `latest.json` for the updater (https://github.com/tauri-apps/tauri-action).
- macOS: needs a paid Apple Developer account for Developer ID and notarization (a free account cannot notarize). CI uses a base64 `.p12` in `APPLE_CERTIFICATE`/`APPLE_CERTIFICATE_PASSWORD` plus `APPLE_SIGNING_IDENTITY`. Notarization credentials are either `APPLE_API_ISSUER`/`APPLE_API_KEY`/`APPLE_API_KEY_PATH` or `APPLE_ID`/`APPLE_PASSWORD`/team id (https://v2.tauri.app/distribute/sign/macos/).
- Windows: the docs cover OV certs (only those acquired before 2023-06-01), Azure Key Vault via relic, a custom `signCommand`, and Azure Artifact Signing (formerly Trusted Signing) via `artifact-signing-cli`. EV no longer gives instant SmartScreen reputation since 2024. Cross-compiling Windows installers from Linux or macOS requires a custom sign command (https://v2.tauri.app/distribute/sign/windows/). One search snippet says Artifact Signing availability is limited to US/Canada individuals and US/Canada/EU/UK organizations. That is from a secondary source [unverified].
- Linux: deb, rpm, AppImage, Flatpak, Snap and AUR are all documented. AppImage is 70+ MB and must be built on the oldest supported base (Ubuntu 22.04 or Debian 12). ARM AppImages cannot be cross-compiled (https://v2.tauri.app/distribute/appimage/). AppImage signing is GPG, but AppImage does not verify it itself (https://v2.tauri.app/distribute/sign/linux/).

**(5) Multiwebview and iframe support per platform for plugin UI**

| | macOS | Windows | Linux |
|---|---|---|---|
| Child webviews (`unstable`) | Works, but arrow-key and double-char input bugs (#10194, #8705) | Works. #8705 notes it does not reproduce on Windows 11 | X11 only, with positioning bugs. Wayland unsupported |
| Separate `WebviewWindow` | Stable | Stable | Stable |
| Iframe in main webview | No IPC bridge injected | Bridge script injected in subframes | No IPC bridge injected |

- Iframe rows are from the wry init-script behaviour described in the "Iframes and IPC per platform" subsection above.
- Real-world precedent: Terminal-64 renders widgets as sandboxed iframes with a `postMessage` bridge and keeps panel chrome in React (https://github.com/Pugbread/Terminal-64). Its own research note concludes "Iframes are browsing contexts, not reliable performance sandboxes" and that WebKit site isolation cannot be relied on. It adds a `native-webview` mode that uses Tauri child webviews plus a Rust broker (https://github.com/Pugbread/Terminal-64/blob/master/docs/widget-isolation-research.md).
- TUICommander loads plugin ES modules in the main webview via `plugin://<id>/main.js` (rewritten to `http://plugin.localhost/...` on Windows because WebView2 only serves custom schemes under http) with 5 capability tiers. This is not a UI sandbox (https://github.com/sstraus/tuicommander/blob/main/docs/plugins.md).

### A.2 GPUI and gpui-component

- `gpui` on crates.io is 0.2.2, last published 2025-10-22 (https://crates.io/crates/gpui). The Zed README says "pre-1.0. There will often be breaking changes between versions" and tells you to add `gpui` plus the `gpui_platform` crate (https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md). `gpui_platform` is NOT published on crates.io (404).
- `gpui-component` 0.7.1 (2026-10-05, 16.5k stars) is published from the renamed repo `longbridge/gpui-kit` (https://crates.io/crates/gpui-component, https://github.com/longbridge/gpui-kit). It depends on third-party snapshot crates `gpui-pre =0.3.8`, `gpui-pre-platform`, `gpui-pre-macros` and `gpui-pre-sum-tree` (https://crates.io/api/v1/crates/gpui-component/0.7.1/dependencies). The owner of `gpui-pre` (0.3.8, 2026-10-05) is user `huacnlee` and the description says "snapshot of zed@279fe07" (https://crates.io/crates/gpui-pre). Its own Cargo.toml says snapshots "may change GPUI's API" (https://github.com/longbridge/gpui-kit/blob/main/Cargo.toml). So, effectively, standalone GPUI means pinned git or community snapshots.
- Zed's terminal uses `alacritty_terminal`: `crates/terminal/Cargo.toml` has `alacritty_terminal.workspace = true`, and the workspace pins Zed's fork by git rev, `zed-industries/alacritty` rev 4c12966... (https://github.com/zed-industries/zed/blob/main/Cargo.toml line 532, https://github.com/zed-industries/zed/blob/main/crates/terminal/Cargo.toml).
- Plugin UI in GPUI:
  - `gpui-webview` (wry-based) is "experimental with limited features". Linux Wayland is not supported (run on XWayland), and on Windows the webview covers GPUI overlays (https://github.com/longbridge/gpui-kit/blob/main/crates/webview/README.md).
  - `gpui-shell` makes a host extensible via JavaScript that describes GPUI elements. "It is not an Electron or a Tauri. There is no WebView, no DOM, no HTML or CSS". It is at "milestone M0: a feasibility baseline, not a stable interface" (https://github.com/longbridge/gpui-kit/blob/main/crates/shell/README.md).
- GPUI terminal apps exist: tty7 (1,229 stars, GPUI + `alacritty_terminal`) and paneflow (83 stars, GPUI + libghostty-vt, macOS/Linux/Windows). tty7 self-reports 2x the throughput of Alacritty, Ghostty and Kitty on an M1 Pro: 11 MB `cat` in 95 ms vs 239 ms (Alacritty) and 179 ms (Ghostty), and DOOM-fire at 888 fps vs 485 and 552 [self] (https://github.com/l0ng-ai/tty7, https://github.com/arthjean/paneflow).

### A.3 Others (brief)

| Framework | Version / date | Notes |
|---|---|---|
| Iced | 0.14.0, 2025-12-07 (https://crates.io/crates/iced) | README: "currently experimental software" (https://github.com/iced-rs/iced). No webview. |
| Slint | 1.18.1, 2026-09-21 (https://crates.io/crates/slint) | Declarative toolkit. Triple license: Royalty-free, GPLv3, or Commercial (https://github.com/slint-ui/slint). |
| Dioxus | 0.7.10 (2026-07-30); 0.8.0-alpha.1 (2026-07-31) (https://crates.io/crates/dioxus) | Desktop renders via webview by default. WGPU and Freya renderers are "experimental" (https://github.com/DioxusLabs/dioxus). Blitz native renderer: `blitz-dom` 0.3.0-beta.2 (2026-08-24), README "beta" (https://github.com/DioxusLabs/blitz). |
| egui | 0.36.2, 2026-09-08 (https://crates.io/crates/egui) | Immediate mode, AccessKit. |
| Makepad | `makepad-widgets` 1.0.0, 2025-05-13 (https://crates.io/crates/makepad-widgets) | Only about 1.3k recent downloads. |
| Electron | 44.7.0, 2026-10-07 (https://github.com/electron/electron/releases) | Bundles Chromium and Node, so the engine is identical on all OSes. Comparison only. |

None of the native-UI options (Iced, Slint, egui, Makepad) have an HTML/JS plugin-UI story. Dioxus and Electron do, because both host webviews.

---

## B. Terminal emulation

### B.1 xterm.js
- `@xterm/xterm` 6.0.0 is the latest, published 2025-12-22. 6.1.0-beta.304 is the newest beta (2026-08-30) and `@xterm/addon-webgl` is 0.19.0 (latest) / 0.20.0-beta.300 (https://registry.npmjs.org/@xterm/xterm, https://registry.npmjs.org/@xterm/addon-webgl). The repo has 21,255 stars and its last push was 2026-09-13 (https://github.com/xtermjs/xterm.js).
- 6.0 added synchronized output (DEC mode 2026), OSC 52, and a new scrollbar, and removed `windowsMode` (https://github.com/xtermjs/xterm.js/releases/tag/6.0.0).
- Official throughput: xterm.js "has a rather low throughput (5 - 35 MB/s)" compared to producers at GB/s. `write` is non-blocking and processes in under one 16 ms frame. The write buffer hard limit is 50 MB and data beyond that is DISCARDED. A watermark/callback flow-control scheme is provided, and a HIGH watermark of 500K or less is recommended for keystroke responsiveness (https://xtermjs.org/docs/guides/flowcontrol/). I found no official frame-rate figures [unverified]. All panes parse on the webview's one main thread [inferred].
- Claude Code context: its docs say it defaults to the alternate-screen "fullscreen" renderer for sessions whose first use was on or after 2026-05-06, with mouse capture. This "reduces the amount of data sent to your terminal on each update". The docs name xterm.js-based terminals (VS Code) in their link-handling note (https://code.claude.com/docs/en/fullscreen). The terminal therefore needs alt-screen, mouse reporting and DEC 2026.
- Uses in Tauri apps: Jean (`@xterm/xterm ^6.0.0`, `addon-fit`, `addon-web-links`, and no WebGL addon in package.json, https://github.com/coollabsio/jean), Terminal-64 ("xterm.js + WebGL"), and many small Claude-Code session managers (e.g. https://github.com/latte3cup/claude-session-manager, https://github.com/zeroisnumber/claude-deck).

### B.2 Ghostty family
- `libghostty-vt` is "already available and usable today for Zig and C and is compatible for macOS, Linux, Windows, and WebAssembly. The functionality is extremely stable... but the API signatures are still in flux." libghostty has no version tag yet (https://github.com/ghostty-org/ghostty README).
- Rust bindings: `libghostty-vt` 0.2.2 (2026-09-28; first published 2026-03-28; about 156k downloads) from `Uzaaft/libghostty-rs` (392 stars). It needs Zig 0.16.x at build time, and the README says it is pre-1.0 and the bindings move with the pinned Ghostty commit (https://crates.io/crates/libghostty-vt, https://github.com/Uzaaft/libghostty-rs). It exposes Terminal, RenderState and Row/CellIterator.
- `ghostty-web` (coder/ghostty-web): npm 0.4.0 (2025-12-09), `next` 0.4.0-next.20 (2026-06-28). It has an xterm.js-compatible API, a WASM parser of about 400 KB, and a Canvas renderer (`lib/renderer.ts`). A WebGL renderer is only a feature request (https://github.com/coder/ghostty-web/issues/155). About 29-30 open issues, and it was created for Coder's Mux (https://github.com/coder/ghostty-web, https://www.npmjs.com/package/ghostty-web).
- Others: `@wterm/core` 0.5.4 (2026-09-29), Vercel Labs experiment, Zig/WASM core, DOM renderer, optional libghostty core (https://github.com/vercel-labs/wterm). `restty` 0.3.0 (2026-09-05), libghostty-vt WASM with WebGPU and a WebGL2 fallback, "early-release software" (https://github.com/wiedymi/restty).
- Maturity [inferred]: usable for experiments, not a drop-in for production on a 12-pane Claude workload without your own testing.

### B.3 Rust VT crates and PTY

| Crate | Version / date | Notes |
|---|---|---|
| `alacritty_terminal` | 0.26.0, 2026-04-06 (https://crates.io/crates/alacritty_terminal) | Apache-2.0. ~1.1M recent downloads. API used by TUICommander: `Term::new`, `Processor::advance`, `term.damage()`/`reset_damage()`, `grid()`, `mode()`, `regex_search_right`, `EventListener`. Includes its own PTY. |
| `wezterm-term` | NOT on crates.io (404) | Workspace crate only. |
| `termwiz` | 0.23.3, 2025-03-20 | |
| `vt100` | 0.16.2, 2025-07-12 | Parser plus in-memory screen (https://github.com/doy/vt100-rust). |
| `avt` | 0.18.0, 2026-05-05 | asciinema's virtual terminal, parser plus buffers only (https://github.com/asciinema/avt). |
| `vte` | 0.15.0, 2025-02-02 | |
| `portable-pty` | 0.9.0, 2025-02-11 | Last release is stale relative to repo activity. |

Crates.io API base is https://crates.io/api/v1/crates/<name>.

### B.4 Raw bytes to xterm.js versus Rust-parsed cell frames to a canvas

- **Raw bytes to xterm.js**
  - Simplest path. A Rust PTY thread coalesces reads and sends `InvokeResponseBody::Raw` through a Channel, with flow control per the xterm guide.
  - Costs: parsing on the webview main thread, the WebGL context cap and WebGL bugs, and the Linux WebGL ambiguity (all detailed above).
- **Rust parser to canvas** (TUICommander is the precedent)
  - Its audit doc dated 2026-09-21 says: "CanvasTerminal is the sole terminal renderer. xterm.js has been fully removed. The renderer is powered by `alacritty_terminal` (Rust) sending binary grid frames over a Tauri Channel (desktop) or WebSocket" (https://github.com/sstraus/tuicommander/blob/main/docs/frontend/canvas-terminal-audit.md).
  - Details from that doc: a 26-byte frame header plus row records and an 11-byte cell core, a base canvas plus an overlay canvas, no frame traffic acknowledged while the pane is hidden (IntersectionObserver flow control), and up to 50 simultaneous sessions with roughly 80 MB RAM (README). These are [self] claims. I found no published benchmark.
  - It had to patch `alacritty_terminal` locally (https://github.com/sstraus/tuicommander/blob/main/docs/backend/alacritty-integration.md). Reasons: "Ink/Claude Code uses CUU cursor positioning that breaks when reflow merges/splits screen lines", so it added `ReflowMode::HistoryOnly`; three pending-wrap and edit-op behaviours diverged from DEC/xterm; plus OSC 133/7 hooks and a second damage view.
  - Takeaways: Claude Code's Ink redraws are sensitive to resize/reflow handling, and you own the renderer, IME, selection, fonts and a11y if you go this way.
  - The Rust side also gets semantic state (agent status, prompt marks), which suits a sidebar.
- A variant: `tauri-plugin-terminal` (0 stars, alacritty_terminal, grid events rendered as DOM) shows the pattern but is not production evidence (https://github.com/Alakazam-211/tauri-plugin-terminal).

### B.5 Existing apps and what they do

| App | Framework | Terminal approach |
|---|---|---|
| opcode (ex-Claudia) | Tauri 2 | No terminal. Runs `claude -p ... --output-format stream-json` headlessly and renders stream messages (https://github.com/winfunc/opcode/blob/main/src-tauri/src/commands/claude.rs lines ~936-947). No xterm dependency in `package.json`. |
| Jean | Tauri 2 + React, `unstable` feature | xterm.js 6 without the WebGL addon (see B.1). |
| Terminal-64 | Tauri 2 | xterm.js + WebGL on a canvas. |
| TUICommander | Tauri 2 + SolidJS | `alacritty_terminal` + canvas (B.4). |
| tty7 | GPUI | GPUI + `alacritty_terminal` (A.2). |
| paneflow | GPUI | GPUI + libghostty-vt (A.2). |

---

## C. Recommendation

**(Framework, terminal) = Tauri 2.12.x on the stable wry runtime, with xterm.js 6 behind a swappable `TerminalView` interface, PTY and VT ownership in Rust, and a pre-planned switch to a Rust-parsed canvas renderer if a benchmark gate fails.**

Strongest evidence:
1. **Plugin UI is a first-class capability only in the webview stack.** Tauri/wry hosts arbitrary HTML/JS (iframes, windows, custom schemes), has capabilities with runtime `add_capability`, and the IPC bridge is main-frame-only except on Windows (wry lib.rs:1029). Working precedents exist: Terminal-64's sandboxed-iframe widgets and TUICommander's `plugin://` loader.
2. **Cross-platform packaging is documented end to end.** macOS notarization, Windows signing (Azure Artifact Signing, custom `signCommand`), Linux deb/rpm/AppImage/Flatpak, and the signed updater, plus `tauri-action` CI. A CEF runtime path exists in v3 alpha as a contingency for the WebKitGTK problem.
3. **A viable native-terminal fallback is proven inside Tauri.** TUICommander removed xterm.js entirely and renders `alacritty_terminal` grid frames over a Tauri Channel, with up to 50 sessions. The fallback matters because xterm.js's WebGL path has real, current risks: a hard 16-context cap in WebKit, an open WKWebView ghosting bug under Claude Code streaming (#5847), and Tauri's own warning that WebGL terminals are the typical silent slow path on WebKitGTK.

**Nearest rival (framework): GPUI (+ gpui-component / gpui-kit) with `alacritty_terminal` or libghostty-vt.** It is the strongest on raw terminal speed: Zed's own terminal uses `alacritty_terminal`, and tty7 self-reports about 2x throughput. It is ruled out for this use case by:
- (a) No HTML/JS plugin-UI story. `gpui-webview` is experimental, has no Wayland support, and on Windows covers overlays. `gpui-shell` is a JS description layer at "M0 feasibility", explicitly "not an Electron or a Tauri".
- (b) The crates.io `gpui` is stale (0.2.2, 2025-10-22), `gpui_platform` is unpublished, and `gpui-component` depends on a third-party `gpui-pre` snapshot series whose own manifest warns of API breaks.

**Nearest rival (terminal): Rust `alacritty_terminal` / libghostty-vt plus a canvas renderer in the webview.** It is not ruled out. It is the planned escape hatch, not the starting point, because:
- You would own selection, IME, fonts, links and accessibility.
- TUICommander needed non-trivial patches to alacritty_terminal for Ink/Claude Code behaviour.
- `libghostty-vt` has API signatures "in flux", needs Zig, and `ghostty-web` is v0.4.0 with Canvas only.
- I found a single production precedent (160 stars) and no independent benchmark.

**Mitigations that the evidence supports**
- Use WebGL only for visible or focused panes (cap at about 8, my number [inferred]) and fall back to the DOM renderer on `onContextLoss`. Default to DOM on Linux until measured, with a user setting as Tauri recommends.
- Apply `__NV_DISABLE_EXPLICIT_SYNC=1` first, and only conditionally set `WEBKIT_DISABLE_DMABUF_RENDERER` / `WEBKIT_DISABLE_COMPOSITING_MODE` (Tauri docs order).
- On the Rust side use `portable-pty` (sideload a current `conpty.dll`/`OpenConsole.exe` pair on Windows), coalesce output, send raw `InvokeResponseBody::Raw` over a Channel, and apply xterm flow control and `windowsPty`.
- Host plugin UI in iframes served by a custom scheme, calling the host over `postMessage` with a Rust-side permission check (grant per plugin via `add_capability`). Offer a separate `WebviewWindow` for heavy plugins. Avoid `unstable` child webviews, especially for text input on macOS.
- Pin the WebView2 version via `fixedRuntime` if consistency matters (+~180 MB). Track `tauri-runtime-cef` as a Linux contingency, but not before it leaves alpha.

**Benchmark gate to run before committing (not done by me)**
1. A 12-pane Claude Code replay (fullscreen mode, alt-screen plus mouse) on WKWebView, WebView2 and WebKitGTK (2.50 and 2.54, X11 and Wayland), comparing xterm DOM, xterm WebGL and Rust-canvas.
2. Channel chunk size and coalescing interval.
3. Iframe vs `WebviewWindow` plugin hosting on each OS.

**Unverified or not measured**
- Any xterm.js frame-rate or MB/s figure per webview; any Tauri Channel MB/s; any Rust-canvas benchmark.
- Chromium's default WebGL context cap (WebKit's 16 is verified).
- Whether the ACL admits IPC from a custom-scheme iframe on Windows (only the script injection was verified).
- Distro WebKitGTK versions end users will have.
- Whether the updater can update deb/rpm installs.
- Azure Artifact Signing regional availability.
- `tauri-runtime-cef` bundle size.