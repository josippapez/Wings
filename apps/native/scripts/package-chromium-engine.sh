#!/bin/sh
# Packages the Chromium engine Wings downloads on demand (`apps/native/src/web/engine.rs`): "Wings Chromium.app", laid out
# as `crates/wings-chromium/src/engine.rs` expects, signed, then archived as a .tar.gz with the bundle folder at its root.
# Run it from the main checkout, through the build queue:
#   sh scripts/build.sh sh apps/native/scripts/package-chromium-engine.sh
# `CEF_PATH` is the exported CEF SDK (default: the spike's, ~/Library/Caches/wings-chromium-spike/sdk). It prints the
# archive's size, unpacked size and SHA-256, which `PINNED` in engine.rs holds. Nothing is published.
set -eu
repo=$(cd "$(dirname "$0")/../../.." && pwd -P)
version=156.0.31
name="wings-chromium-$version-darwin-aarch64.tar.gz"
framework="Chromium Embedded Framework.framework"
helper="Wings Chromium Helper"
out="$repo/apps/native/target/chromium-engine"

if [ -z "${CEF_PATH:-}" ] && [ -d "$HOME/Library/Caches/wings-chromium-spike/sdk" ]; then
  CEF_PATH="$HOME/Library/Caches/wings-chromium-spike/sdk"
fi
if [ -z "${CEF_PATH:-}" ] || [ ! -d "$CEF_PATH/$framework" ]; then
  echo "Set CEF_PATH to the exported CEF SDK (the folder holding $framework)." >&2
  exit 1
fi
export CEF_PATH

cargo build --release --manifest-path "$repo/crates/wings-chromium/Cargo.toml" --bin wings-chromium-helper --target-dir "$repo/apps/native/target"
binary="$repo/apps/native/target/release/wings-chromium-helper"

# The same choice as build-mac-app.sh: a valid Apple Development certificate, else the local one, else ad hoc.
IDENTITY=-
APPLE=$(security find-identity -v -p codesigning | awk '/"Apple Development: / { print $2; exit }')
if [ -n "$APPLE" ]; then
  IDENTITY=$APPLE
elif security find-identity -p codesigning | grep -q '"Wings Local Signing"'; then
  IDENTITY="Wings Local Signing"
fi

plist() { # <path> <executable> <bundle id>
  cat > "$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>$2</string>
  <key>CFBundleIdentifier</key><string>$3</string>
  <key>CFBundleName</key><string>$2</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSUIElement</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
</dict>
</plist>
PLIST
}

work=$(mktemp -d "${TMPDIR:-/tmp}/wings-chromium-engine.XXXXXX")
trap 'rm -rf "$work"' EXIT
engine="$work/Wings Chromium.app"
frameworks="$engine/Contents/Frameworks"
mkdir -p "$frameworks"
# A bundle without an executable: Chromium's child sandbox reads inside one bundle, and this Info.plist gives the bundle
# ID the browser process and every child share.
plist "$engine/Contents/Info.plist" "Wings Chromium" dev.wings.chromium
cp -cR "$CEF_PATH/$framework" "$frameworks/"
for suffix in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
  app="$frameworks/$helper$suffix.app"
  mkdir -p "$app/Contents/MacOS"
  cp "$binary" "$app/Contents/MacOS/$helper$suffix"
  id="dev.wings.chromium.helper$(echo "$suffix" | tr -d ' ()' | tr 'A-Z' 'a-z' | sed 's/^./.&/')"
  plist "$app/Contents/Info.plist" "$helper$suffix" "$id"
done
# No stray files from the SDK's export.
find "$engine" \( -name .DS_Store -o -name '._*' \) -delete

codesign --force --deep --sign "$IDENTITY" "$engine"
codesign --verify --deep --strict "$engine"
if find "$engine" -type l | grep -q .; then
  echo "The bundle holds symbolic links, which the installer refuses." >&2
  exit 1
fi

mkdir -p "$out"
archive="$out/$name"
rm -f "$archive"
# Folder and file entries only, with no extended attributes or AppleDouble files.
(cd "$work" && COPYFILE_DISABLE=1 tar --no-xattrs -czf "$archive" "Wings Chromium.app")

size=$(stat -f %z "$archive")
unpacked=$(find "$engine" -type f -exec stat -f %z {} + | awk '{ total += $1 } END { print total }')
sha=$(shasum -a 256 "$archive" | awk '{ print $1 }')
echo "archive:  $archive"
echo "size:     $size"
echo "unpacked: $unpacked"
echo "sha256:   $sha"
echo "identity: $IDENTITY"
