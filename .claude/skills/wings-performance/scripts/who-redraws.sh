#!/bin/sh
# Who asks the running Wings to draw again: attaches lldb for a few seconds, records the stack of each
# App::notify and WindowInvalidator::invalidate_view, detaches, and prints the callers that did it most.
#   sh who-redraws.sh [seconds, default 6]
# Wings pauses a few ms per hit while attached. It always detaches; check `ps -o stat= -p <pid>` shows no X after.
set -eu
secs=${1:-6}
here=$(cd "$(dirname "$0")" && pwd)
bin=/Applications/Wings.app/Contents/MacOS/wings-native
pid=$(pgrep -f MacOS/wings-native | head -1)
out=$(mktemp -d)
notify=$(nm "$bin" | grep -o '_RNvMsc_NtCs[0-9A-Za-z_]*_4gpui3appNtB5_3App6notify$' | head -1)
invalidate=$(nm "$bin" | grep -o '_RNvMs_NtCs[0-9A-Za-z_]*_4gpui6windowNtB4_17WindowInvalidator15invalidate_view$' | head -1)
cat > "$out/cmds" <<CMDS
command script import $here/stacks.py
breakpoint set -s wings-native -n $notify
breakpoint set -s wings-native -n $invalidate
breakpoint command add -F stacks.hit 1
breakpoint command add -F stacks.hit 2
continue
CMDS
WINGS_STACKS="$out/stacks.txt" lldb -p "$pid" --batch -s "$out/cmds" -o 'process detach' > "$out/lldb.log" 2>&1 &
sleep "$secs"
# The first interrupt stops `continue`; a second is sometimes needed before lldb runs the detach.
for _ in 1 2 3; do pkill -INT -f "lldb -p $pid" || true; sleep 3; pgrep -f "lldb -p $pid" >/dev/null || break; done
python3 "$here/stacks.py" "$out/stacks.txt" "$secs"
