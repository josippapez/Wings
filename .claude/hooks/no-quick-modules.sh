#!/bin/sh
# Refuses a commit that holds a plugin module `build-rust.sh --quick` wrote: Wings installs plugins straight from main,
# and a quick module is bigger and slower than the one it should ship. `plugins/target/quick-built` lists them until a
# full build of the plugin replaces them.
input=$(cat)
command=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
printf '%s' "$command" | grep -Eq '(^|[^[:alnum:]_-])git[[:space:]](.*[[:space:]])?commit([[:space:]]|$)' || exit 0
root=${CLAUDE_PROJECT_DIR:-$(pwd)}
list="$root/plugins/target/quick-built"
[ -s "$list" ] || exit 0
# What the commit takes: what is staged, and with -a or --all every changed file too.
changed=$(git -C "$root" diff --cached --name-only)
if printf '%s' "$command" | grep -Eq '[[:space:]](-[[:alnum:]]*a[[:alnum:]]*|--all)([[:space:]]|$)'; then
  changed="$changed
$(git -C "$root" diff --name-only)"
fi
quick=$(printf '%s\n' "$changed" | grep -Fxf "$list" | sed 's/^/  /')
[ -n "$quick" ] || exit 0
plugins=$(printf '%s\n' "$quick" | sed -E 's|^  plugins/([^/]+)/.*|\1|' | sort -u | tr '\n' ' ')
jq -n --arg why "These modules are quick builds, bigger and slower than Wings should ship:
$quick
Build them for real first: sh scripts/build.sh sh plugins/build-rust.sh ${plugins% }" \
  '{ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $why } }'
