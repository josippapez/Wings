#!/bin/sh
# Builds and signs the local native app bundle. Installation is separate.
set -e
cd "$(dirname "$0")/.."
cargo build --release --bins
APP=target/release/Wings-native.app
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# The plugin worker is found next to the app's own binary, and Claude Code's settings point at the `wings` command
# beside it.
cp target/release/wings-native target/release/wings-plugin-worker target/release/wings target/release/wings-terminal-server "$APP/Contents/MacOS/"
cp ../desktop/src-tauri/icons/icon.icns "$APP/Contents/Resources/icon.icns"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Wings-native</string>
  <key>CFBundleDisplayName</key><string>Wings-native</string>
  <key>CFBundleIdentifier</key><string>dev.wings.native</string>
  <key>CFBundleExecutable</key><string>wings-native</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
IDENTITY=-
if security find-identity -p codesigning | grep -q '"Wings Local Signing"'; then
  IDENTITY="Wings Local Signing"
fi
codesign --force --deep --sign "$IDENTITY" "$APP"
