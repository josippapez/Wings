#!/bin/sh
# Builds and test runs happen in the main checkout's target folders: each one a subagent ran in its own place made
# another multi-GB target folder and filled the disk. A subagent builds through `scripts/build.sh`, which runs up to two
# at once in the main checkout and queues the rest; any other build command of a subagent is refused. What counts as a
# build, and what only mentions one, is in builds-in-one-place.py.
input=$(cat)
[ -n "$(printf '%s' "$input" | jq -r '.agent_id // empty')" ] || exit 0
command=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
verdict=$(printf '%s' "$command" | python3 -I "$(dirname "$0")/builds-in-one-place.py")
if [ "$verdict" = deny ]; then
  jq -n '{ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "deny",
    permissionDecisionReason: "Build and test through the queue, from the main checkout: sh scripts/build.sh <command>, for example sh scripts/build.sh cargo test --manifest-path apps/native/Cargo.toml --features test-support --lib <filter>. It waits for any build already running." } }'
fi
