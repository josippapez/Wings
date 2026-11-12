#!/bin/sh
# Builds the plugins written in Rust: each frame a plugin's manifest names as a .wasm (its main, its pages) is the
# crate in <plugin>/rust/ built with the feature of that frame's name, written to <plugin>/dist/native/<frame>.wasm.
# The output is committed, because Wings installs a plugin folder straight from the default branch.
#   sh build-rust.sh              every plugin with a rust/ folder
#   sh build-rust.sh dev-servers  one plugin
#   sh build-rust.sh --check      fail if a committed module differs from what rust/ builds
# Needs the target: rustup target add wasm32-unknown-unknown.
set -eu
cd "$(dirname "$0")"
check=""
if [ "${1:-}" = --check ]; then
  check=1
  shift
fi
[ "$#" -gt 0 ] || set -- $(for dir in */rust; do dirname "$dir"; done)
stale=0
for plugin in "$@"; do
  crate=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$plugin/rust/Cargo.toml" | head -n 1)
  frames=$(jq -r '[.main, (.contributes | (.sidebars[]? | .page, .popover.page?), .views[]?.page, .blocks[]?.page, .settings?.page?)] | .[] | select(type == "string" and endswith(".wasm"))' "$plugin/wings-plugin.json")
  for frame in $frames; do
    name=$(basename "$frame" .wasm)
    cargo build --quiet --release --target wasm32-unknown-unknown -p "$crate" --features "$name"
    built="target/wasm32-unknown-unknown/release/$(echo "$crate" | tr - _).wasm"
    if [ -n "$check" ]; then
      cmp -s "$built" "$plugin/$frame" || { echo "$plugin/$frame is out of date. Run \`sh build-rust.sh $plugin\` in plugins/ and commit it." >&2; stale=1; }
    else
      mkdir -p "$(dirname "$plugin/$frame")"
      cp "$built" "$plugin/$frame"
      echo "$plugin/$frame  $(($(wc -c < "$plugin/$frame") / 1024)) KB"
    fi
  done
done
exit "$stale"
