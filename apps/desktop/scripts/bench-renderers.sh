#!/bin/sh
# Renderer benchmark in the real WKWebView: the Wings UI with no terminals, then a recorded Claude Code session
# replayed into 1, 4 and 12 panes (and 12 with 4 shown) with xterm.js DOM, xterm.js WebGL, and the Rust-parsed canvas
# prototype (one canvas per pane, and one shared). Each run opens a Wings window, which takes focus, and quits itself;
# about 3 to 4 minutes in all. Writes one JSON report per run to the folder given (default bench-results) and prints
# a table.
#
# Build first: pnpm tauri build --no-bundle --features bench-canvas
set -eu
cd "$(dirname "$0")/.."
bin=src-tauri/target/release/wings
out=${1:-bench-results}
mkdir -p "$out"

run() {
  name=$1
  shift
  echo "running $name"
  env "$@" WINGS_BENCH_OUT="$out/$name.json" "$bin" >/dev/null 2>&1 &
  pid=$!
  (sleep 150 && kill "$pid" 2>/dev/null) &
  watchdog=$!
  wait "$pid" || echo "$name did not finish"
  kill "$watchdog" 2>/dev/null || true
}

run empty-ui WINGS_BENCH=empty
for renderer in dom webgl canvas canvas-shared; do
  run "$renderer" WINGS_BENCH=replay WINGS_BENCH_RENDERER="$renderer"
done
python3 scripts/bench-table.py "$out"/*.json
