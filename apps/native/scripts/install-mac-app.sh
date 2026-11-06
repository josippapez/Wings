#!/bin/sh
# Builds the native app and installs it as /Applications/Wings-native.app, beside the desktop app, then opens
# it. Signed like the desktop app with the "Wings Local Signing" certificate when there is one, so macOS keeps
# its privacy approvals across builds; ad hoc otherwise.
set -e
cd "$(dirname "$0")/.."
sh scripts/build-mac-app.sh
APP=target/release/Wings-native.app
/usr/bin/osascript -e 'tell application id "dev.wings.native" to quit' >/dev/null 2>&1 || true
while pgrep -f "^/Applications/Wings-native.app/Contents/MacOS/wings-native" >/dev/null; do sleep 0.2; done
rm -rf /Applications/Wings-native.app
cp -R "$APP" /Applications/
open /Applications/Wings-native.app
echo "Installed /Applications/Wings-native.app"
