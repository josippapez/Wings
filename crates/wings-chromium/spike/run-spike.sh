#!/bin/sh
# Opens the spike by hand: the host window with the browser glued over it and Bitwarden loaded.
#   sh crates/wings-chromium/spike/run-spike.sh [extra spike or Chromium switches]
# What to try:
#   - Click in the page, type; click in the "Terminal stand-in" field, type: each takes the typing in turn, and the host
#     window's traffic lights should stay lit while the page has focus.
#   - Move and resize the host window: the page should follow without lag or gaps.
#   - Click "Bitwarden": its popup opens under the button; click again to close it.
#   - Cmd-` and Mission Control: the page shouldn't show as a window of its own.
#   - Quit with Cmd-Q (CEF's shutdown then hangs up to 10 s before Chromium's watchdog ends the process).
# --use-mock-keychain keeps Chromium's cookie key out of the Keychain: every rebuild of the ad-hoc signed spike is a new
# app to the Keychain, which would ask before handing the key over, and every navigation waits until someone answers.
# Commands also run without input: write them to the control file and send SIGUSR1 (see run_command in spike.rs):
#   echo popup > ~/Library/Caches/wings-chromium-spike/run/control; pkill -USR1 -f wings-chromium-spike
# Worth comparing by hand: `never-main` (the page's window can't become main, so the host's traffic lights should stay
# lit while you type in the page) and `borderless` (drops the titled style mask CEF keeps, and with it the rounded
# corners at the page's edges). `state`, `geometry` and `order` print what AppKit reports.
set -eu
spike="$HOME/Library/Caches/wings-chromium-spike"
[ -x "$spike/Wings Chromium Spike.app/Contents/MacOS/wings-chromium-spike" ] || sh "$(dirname "$0")/assemble.sh"
mkdir -p "$spike/run"
: > "$spike/run/control"
exec "$spike/Wings Chromium Spike.app/Contents/MacOS/wings-chromium-spike" \
  --wcs-control="$spike/run/control" \
  --remote-debugging-port=9333 \
  --load-extension="$spike/extensions/bitwarden" \
  --use-mock-keychain \
  "$@"
