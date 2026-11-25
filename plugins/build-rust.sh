#!/bin/sh
# Builds the plugins written in Rust: each frame a plugin's manifest names as a .wasm (its main, its pages) is the
# crate in <plugin>/rust/ built with the feature of that frame's name, written to <plugin>/dist/native/<frame>.wasm.
# The output is committed, because Wings installs a plugin folder straight from the default branch.
#   sh build-rust.sh              every plugin with a rust/ folder
#   sh build-rust.sh dev-servers  one plugin
#   sh build-rust.sh --check      fail if a committed module differs from what rust/ builds
#   sh build-rust.sh --quick <plugin>   build it in about a tenth of the time, to try a change or take screenshots. The
#                                 modules are bigger and slower: build it again without --quick before you commit,
#                                 which `.claude/hooks/no-quick-modules.sh` makes sure of.
# Needs the target: rustup target add wasm32-unknown-unknown.
set -eu
cd "$(dirname "$0")"
check=""
profile=release
suffix=""
case "${1:-}" in
  --check) check=1; shift ;;
  --quick) profile=quick; suffix=" (quick)"; shift ;;
esac
# The modules --quick wrote, which a commit refuses.
quick=target/quick-built
[ "$#" -gt 0 ] || set -- $(for dir in */rust; do dirname "$dir"; done)
stale=0
for plugin in "$@"; do
  crate=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$plugin/rust/Cargo.toml" | head -n 1)
  frames=$(jq -r '[.main, (.contributes | (.sidebars[]? | .page, .popover.page?), .views[]?.page, .blocks[]?.page, .settings?.page?)] | .[] | select(type == "string" and endswith(".wasm"))' "$plugin/wings-plugin.json")
  for frame in $frames; do
    name=$(basename "$frame" .wasm)
    cargo build --quiet --profile "$profile" --target wasm32-unknown-unknown -p "$crate" --features "$name"
    built="target/wasm32-unknown-unknown/$profile/$(echo "$crate" | tr - _).wasm"
    if [ -n "$check" ]; then
      cmp -s "$built" "$plugin/$frame" || { echo "$plugin/$frame is out of date. Run \`sh build-rust.sh $plugin\` in plugins/ and commit it." >&2; stale=1; }
    else
      mkdir -p "$(dirname "$plugin/$frame")"
      cp "$built" "$plugin/$frame"
      grep -vFx "plugins/$plugin/$frame" "$quick" > "$quick.new" 2>/dev/null || true
      [ "$profile" = quick ] && echo "plugins/$plugin/$frame" >> "$quick.new"
      mv "$quick.new" "$quick"
      echo "$plugin/$frame  $(($(wc -c < "$plugin/$frame") / 1024)) KB$suffix"
    fi
  done
done
exit "$stale"
