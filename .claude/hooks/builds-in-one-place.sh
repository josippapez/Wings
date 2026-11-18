#!/bin/sh
# Builds and test runs belong to the main session, in the main checkout: each one a subagent ran in its own place made
# another multi-GB target folder and filled the disk. A subagent writes code and hands it back.
input=$(cat)
[ -n "$(printf '%s' "$input" | jq -r '.agent_id // empty')" ] || exit 0
command=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
if printf '%s' "$command" | grep -Eq '(^|[^[:alnum:]_-])(cargo[[:space:]]+(build|test|run|check|clippy|bench|nextest)|build-rust\.sh|package-mac-app\.sh|pnpm([[:space:]]+--dir[[:space:]]+[^[:space:]]+)?[[:space:]]+(shot|test|build))'; then
  jq -n '{ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "deny",
    permissionDecisionReason: "Builds and test runs happen only in the main session, in the main checkout. Write the code and hand back what to build and test." } }'
fi
