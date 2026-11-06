#!/bin/sh
# Stages a native archive and authenticated feed. It does not install or publish it.
set -e
cd "$(dirname "$0")/.."

case "$(uname -s)" in
  Darwin) ;;
  *) echo "Native packaging runs on macOS only" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  arm64) PLATFORM=darwin-aarch64 ;;
  x86_64) PLATFORM=darwin-x86_64 ;;
  *) echo "Unsupported macOS architecture: $(uname -m)" >&2; exit 1 ;;
esac

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
DIR="target/release/bundle/macos/$PLATFORM"
ARCHIVE="Wings-native-$VERSION-$PLATFORM.app.tar.gz"
FEED="native-latest-$PLATFORM.json"
mkdir -p "$DIR"
# Never leave a publishable old feed after a failed signing/build attempt.
rm -f "$DIR/$ARCHIVE" "$DIR/$FEED" "$DIR/$FEED.sig" "$DIR/native-release.json"
KEY="${WINGS_UPDATER_KEY:-$HOME/.tauri/wings-updater.key}"
[ -f "$KEY" ] || { echo "No updater key at $KEY. Set WINGS_UPDATER_KEY to the native updater signing key." >&2; exit 1; }
KEY="$(cd "$(dirname "$KEY")" && pwd)/$(basename "$KEY")"
sh scripts/build-mac-app.sh
COPYFILE_DISABLE=1 tar -czf "$DIR/$ARCHIVE" -C target/release Wings-native.app
SHA256=$(shasum -a 256 "$DIR/$ARCHIVE" | awk '{print $1}')
SIZE=$(wc -c < "$DIR/$ARCHIVE" | tr -d ' ')
NOTES="${1-}"
export VERSION PLATFORM ARCHIVE SHA256 SIZE NOTES
# Sign a temporary file, then expose the feed only after both files exist.
TMP="$DIR/$FEED.pending"
trap 'rm -f "$TMP" "$TMP.sig"' EXIT HUP INT TERM
node -e 'const {VERSION,PLATFORM,ARCHIVE,SHA256,SIZE,NOTES}=process.env; require("fs").writeFileSync(process.argv[1], JSON.stringify({bundle_identifier:"dev.wings.native",version:VERSION,platform:PLATFORM,artifact:ARCHIVE,size:Number(SIZE),sha256:SHA256,notes:NOTES})+"\n")' "$TMP"
(cd ../desktop && pnpm tauri signer sign --private-key-path "$KEY" --password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD-}" "../native/$TMP")
[ -s "$TMP.sig" ] || { echo "Signing did not produce the native feed signature" >&2; exit 1; }
mv "$TMP.sig" "$DIR/$FEED.sig"
mv "$TMP" "$DIR/$FEED"
echo "Packaged $DIR/$ARCHIVE and signed $FEED"
echo "Publish all three files in a GitHub release tagged native-v$VERSION in josippapez/Wings. Include both architecture feeds in the latest release when shipping both."
