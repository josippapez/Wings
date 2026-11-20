#!/bin/sh
# Runs one build or test command in the main checkout, one at a time across every session and agent, in the one set of
# target folders (`apps/native/target`, `plugins/target`): each place that built on its own made a target folder of
# several GB and filled the disk. A command waits for the build before it to finish.
#   sh scripts/build.sh cargo test --manifest-path apps/native/Cargo.toml --features test-support --lib <filter>
#   sh scripts/build.sh sh plugins/build-rust.sh claude-chat
#   sh scripts/build.sh pnpm --dir plugins shot /tmp/shots --claude-chat
set -eu
# The main checkout, from git, whichever copy of this script runs: a worktree's own copy builds there too.
root=$(cd "$(git -C "$(dirname "$0")" rev-parse --path-format=absolute --git-common-dir)/.." && pwd -P)
for place in "$(pwd -P)" "$(cd "$(dirname "$0")" && pwd -P)"; do
  case "$place/" in
    "$root"/.claude/worktrees/*) echo "build.sh: run it from the main checkout, $root, not a worktree" >&2; exit 2 ;;
    "$root"/*) ;;
    *) echo "build.sh: run it inside the main checkout, $root" >&2; exit 2 ;;
  esac
done
[ "$#" -gt 0 ] || { echo "usage: sh scripts/build.sh <command> [args...]" >&2; exit 2; }
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  echo "build.sh: unset CARGO_TARGET_DIR: builds share the checkout's target folders" >&2
  exit 2
fi

lock="$root/.build.lock"
waited=0
until mkdir "$lock" 2>/dev/null; do
  holder=$(cat "$lock/pid" 2>/dev/null || true)
  # A build that ended without letting go (killed, the machine slept) leaves its lock behind.
  if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
    rm -rf "$lock"
    continue
  fi
  [ "$waited" -eq 0 ] && echo "build.sh: waiting for the build already running (pid ${holder:-?})" >&2
  waited=$((waited + 1))
  sleep 2
done
echo $$ > "$lock/pid"
trap 'rm -rf "$lock"' EXIT INT TERM

# Under 8 GB free, drop the build caches first: the next build makes them again.
free_kb=$(df -k "$root" | awk 'NR==2 {print $4}')
if [ "$free_kb" -lt 8388608 ]; then
  echo "build.sh: under 8 GB free, deleting the incremental build caches" >&2
  rm -rf "$root/apps/native/target/debug/incremental" "$root/plugins/target/debug/incremental"
fi

cd "$root"
"$@"
