#!/bin/sh
# Lays out the spike as Wings would have it after downloading the engine:
#   ~/Library/Caches/wings-chromium-spike/156.0.31/Wings Chromium.app   the engine: CEF's framework and the helpers
#   ~/Library/Caches/wings-chromium-spike/Wings Chromium Spike.app   the host app, with no CEF in it
#   ~/Library/Caches/wings-chromium-spike/extensions/bitwarden       a copy of Chrome's Bitwarden
# Build first, from the main checkout:
#   CEF_PATH=~/Library/Caches/wings-chromium-spike/sdk sh scripts/build.sh cargo build \
#     --manifest-path crates/wings-chromium/Cargo.toml --target-dir apps/native/target --bins --examples
set -eu
repo=$(cd "$(dirname "$0")/../../.." && pwd -P)
spike="$HOME/Library/Caches/wings-chromium-spike"
sdk="$spike/sdk"
engine="$spike/156.0.31/Wings Chromium.app"
frameworks="$engine/Contents/Frameworks"
build="$repo/apps/native/target/debug"
framework="Chromium Embedded Framework.framework"
helper="Wings Chromium Helper"

plist() { # <path> <executable> <bundle id> <ui element 0|1>
  cat > "$1" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>$2</string>
  <key>CFBundleIdentifier</key><string>$3</string>
  <key>CFBundleName</key><string>$2</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSUIElement</key><$( [ "$4" = 1 ] && echo true || echo false )/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
</dict>
</plist>
EOF
}

mkdir -p "$frameworks"
# The engine is a bundle without an executable: Chromium's child sandbox reads inside one bundle, and its Info.plist
# gives the bundle ID the browser process and the children share.
plist "$engine/Contents/Info.plist" "Wings Chromium" app.wings.chromium-spike.engine 1
# The framework, cloned (APFS copy-on-write: no extra space) from the exported SDK, standing in for the download.
if [ ! -d "$frameworks/$framework" ]; then
  cp -cR "$sdk/$framework" "$frameworks/"
fi
for suffix in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
  app="$frameworks/$helper$suffix.app"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS"
  cp "$build/wings-chromium-helper" "$app/Contents/MacOS/$helper$suffix"
  id=$(printf 'app.wings.chromium-spike.helper%s' "$(echo "$suffix" | tr -d ' ()' | tr 'A-Z' 'a-z' | sed 's/^./.&/')")
  plist "$app/Contents/Info.plist" "$helper$suffix" "$id" 1
  codesign --force --sign - "$app" 2>/dev/null
done

host="$spike/Wings Chromium Spike.app"
rm -rf "$host"
mkdir -p "$host/Contents/MacOS"
cp "$build/examples/spike" "$host/Contents/MacOS/wings-chromium-spike"
plist "$host/Contents/Info.plist" wings-chromium-spike app.wings.chromium-spike 0
codesign --force --sign - "$host" 2>/dev/null

# Bitwarden from Chrome, copied (never changed in place). `_metadata` is the store's signature data; Chrome refuses to
# load an unpacked extension with a top-level name starting with "_" that it doesn't know.
extensions="$HOME/Library/Application Support/Google/Chrome/Default/Extensions/nngceckbapebfimnlniiiahkandclblb"
version=$(ls "$extensions" | sort | tail -1)
if [ ! -d "$spike/extensions/bitwarden" ]; then
  mkdir -p "$spike/extensions"
  cp -cR "$extensions/$version" "$spike/extensions/bitwarden"
  rm -rf "$spike/extensions/bitwarden/_metadata"
fi

echo "engine:    $engine"
echo "host app:  $host (CEF inside: $(find "$host" -iname '*chromium embedded*' | wc -l | tr -d ' ') files)"
echo "extension: $spike/extensions/bitwarden (Bitwarden $version)"
