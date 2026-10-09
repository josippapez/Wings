#!/bin/sh
# Builds Wings, then quits the running copy, puts the new one in /Applications and opens it. The swap runs
# in its own session, so it finishes even when this script was started from a Wings pane.
set -e
cd "$(dirname "$0")/.."
sh scripts/build-mac-app.sh
NEW="$PWD/src-tauri/target/release/bundle/macos/Wings.app"
/usr/bin/perl -MPOSIX -e 'POSIX::setsid(); exec @ARGV' /bin/sh -c '
  /usr/bin/osascript -e "tell application id \"dev.wings.app\" to quit" >/dev/null 2>&1 || true
  while pgrep -f "^/Applications/Wings.app/Contents/MacOS/wings$" >/dev/null; do sleep 0.2; done
  rm -rf /Applications/Wings.app && cp -R "$1" /Applications/ && open /Applications/Wings.app
' sh "$NEW" >/dev/null 2>&1 </dev/null &
echo "Built. Wings will quit, update and reopen."
