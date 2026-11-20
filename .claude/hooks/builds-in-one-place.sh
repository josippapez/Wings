#!/bin/sh
# Builds and test runs happen in the main checkout, one at a time: each one a subagent ran in its own place made another
# multi-GB target folder and filled the disk. A subagent builds through `scripts/build.sh`, which queues it behind any
# build already running and runs it in the main checkout; any other build command of a subagent is refused.
input=$(cat)
[ -n "$(printf '%s' "$input" | jq -r '.agent_id // empty')" ] || exit 0
command=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
verdict=$(printf '%s' "$command" | python3 -I -c '
import re, sys
build = re.compile(r"(^|[^\w-])(cargo\s+(build|test|run|check|clippy|bench|nextest)|build-rust\.sh|package-mac-app\.sh|pnpm(\s+--dir\s+\S+)?\s+(shot|test|build))")
queued = re.compile(r"^\s*(sh\s+)?(\S*/)?scripts/build\.sh(\s|$)")
command = sys.stdin.read()
# A substitution or a second line runs a command a plain split cannot see: with one, any build is refused.
hidden = re.search(r"\$\(|`|\n|<\(", command)
for part in re.split(r"&&|\|\||;|\||&", command):
    if build.search(part) and (hidden or not queued.match(part)):
        print("deny")
        break
')
if [ "$verdict" = deny ]; then
  jq -n '{ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "deny",
    permissionDecisionReason: "Build and test through the queue, from the main checkout: sh scripts/build.sh <command>, for example sh scripts/build.sh cargo test --manifest-path apps/native/Cargo.toml --features test-support --lib <filter>. It waits for any build already running." } }'
fi
