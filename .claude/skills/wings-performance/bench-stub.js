// Load after tauri-stub.js to run the renderer benchmark (src/bench-replay.ts) in Chromium:
// http://localhost:1420/?renderer=webgl runs one renderer (?suite=empty the UI with no terminals) and leaves the report in window.__benchReport and #bench-report.
// Needs bench/claude-fullscreen.frames, made by `cargo run --release --manifest-path src-tauri/bench-grid/Cargo.toml
// -- bench/claude-fullscreen.rec bench/claude-fullscreen.frames` in apps/desktop.
(() => {
  const inner = window.__TAURI_INTERNALS__.invoke;
  const params = new URLSearchParams(location.search);
  const renderer = params.get("renderer") ?? "webgl";
  const proc = { pid: 0, cpuMs: 0 };
  const answers = {
    bench_mode: () => true,
    bench_config: () => ({ suite: params.get("suite") ?? "replay", renderer, native: false }),
    // No processes to measure in the browser.
    bench_sample: () => ({ wings: proc, webContent: null, gpu: null }),
    bench_report: ({ report }) => {
      window.__benchReport = JSON.parse(report);
      // Readable with `agent-browser get text "#bench-report"`.
      const pre = document.createElement("pre");
      pre.id = "bench-report";
      pre.textContent = JSON.stringify(window.__benchReport);
      document.body.replaceChildren(pre);
    },
  };
  window.__TAURI_INTERNALS__.invoke = (cmd, args, options) => (cmd in answers ? Promise.resolve(answers[cmd](args)) : inner(cmd, args, options));
})();
