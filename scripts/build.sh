#!/bin/sh
# Runs a build or test command in the main checkout, at most two at a time across every session and agent, in the one
# set of target folders (`apps/native/target`, `plugins/target`): each place that built on its own made a target folder
# of several GB and filled the disk. Cargo locks a target folder only while it compiles, so two commands in the same
# folder take turns compiling and then run their tests or screenshots side by side. A third waits for one to finish.
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

# Two slots. The first keeps the name the one-at-a-time script used, so a copy of it still running counts.
slots="$root/.build.lock $root/.build.lock.2"
# Whether `lock` is held by a build that is still running. One that ended without letting go (killed, the machine
# slept) leaves its lock behind, which goes.
held_by_running() {
  [ -d "$1" ] || return 1
  holder=$(cat "$1/pid" 2>/dev/null || true)
  # Just taken, its pid not written yet.
  [ -n "$holder" ] || return 0
  kill -0 "$holder" 2>/dev/null && return 0
  rm -rf "$1"
  return 1
}
lock=""
waited=0
until [ -n "$lock" ]; do
  for slot in $slots; do
    if mkdir "$slot" 2>/dev/null || { ! held_by_running "$slot" && mkdir "$slot" 2>/dev/null; }; then
      lock=$slot
      break
    fi
  done
  if [ -z "$lock" ]; then
    [ "$waited" -eq 0 ] && echo "build.sh: waiting, two builds are already running" >&2
    waited=1
    sleep 2
  fi
done
echo $$ > "$lock/pid"
trap 'rm -rf "$lock"' EXIT INT TERM

# Whether the other slot holds a build that is still running.
other_running() {
  for slot in $slots; do
    [ "$slot" = "$lock" ] || ! held_by_running "$slot" || return 0
  done
  return 1
}

# The build output nothing uses any more: old builds, old caches and unlinked object files (`scripts/prune-builds.py`).
prune() {
  (cd "$root" && python3 -I scripts/prune-builds.py "$@" apps/native/target plugins/target) ||
    echo "build.sh: couldn't remove the old build output" >&2
}
free_kb() { df -k "$root" | awk 'NR==2 {print $4}'; }

# Under 8 GB free, remove the old build output, and if that isn't enough, drop the build caches: the next build makes
# them again, but compiles a one-line change of the app in 55 s instead of 16 s. Not while the other slot builds, which
# may be compiling with the caches.
if [ "$(free_kb)" -lt 8388608 ]; then
  if other_running; then
    prune --busy
    echo "build.sh: under 8 GB free, but another build is running, so the build caches stay" >&2
  else
    prune
    if [ "$(free_kb)" -lt 8388608 ]; then
      echo "build.sh: under 8 GB free, deleting the incremental build caches" >&2
      rm -rf "$root/apps/native/target/debug/incremental" "$root/plugins/target/debug/incremental"
    fi
  fi
fi

cd "$root"
status=0
"$@" || status=$?
if other_running; then prune --busy; else prune; fi
exit "$status"
