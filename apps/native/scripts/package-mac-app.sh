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

# WINGS_SWITCH=1 packages the replacement Wings.app (scripts/build-mac-app.sh): its own feed, plus the desktop's
# latest.json, so the desktop's updater installs it in the desktop's place.
if [ "${WINGS_SWITCH-}" = 1 ]; then
  VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' ../desktop/src-tauri/Cargo.toml | head -n 1)
  BUNDLE=Wings.app IDENTIFIER=dev.wings.app TAG="v$VERSION"
  ARCHIVE="Wings-$VERSION-$PLATFORM.app.tar.gz"
  FEED_BASE="wings-latest-$PLATFORM"
else
  VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
  BUNDLE=Wings-native.app IDENTIFIER=dev.wings.native TAG="native-v$VERSION"
  ARCHIVE="Wings-native-$VERSION-$PLATFORM.app.tar.gz"
  FEED_BASE="native-latest-$PLATFORM"
fi
# Two feeds: the first as apps up to 0.2.10 read it (they refuse a field they don't know), and the second with the
# terminal server's version too, which newer apps read (`updater::FEED_V2`) to tell whether the update brings a newer
# server.
FEED="$FEED_BASE.json"
FEED2="$FEED_BASE.v2.json"
SERVER_VERSION=$(sed -n 's/^pub const SERVER_VERSION: u32 = \([0-9][0-9]*\);$/\1/p' ../../crates/wings-ptyd/src/lib.rs)
[ -n "$SERVER_VERSION" ] || { echo "Couldn't read SERVER_VERSION from crates/wings-ptyd/src/lib.rs" >&2; exit 1; }
DIR="target/release/bundle/macos/$PLATFORM"
mkdir -p "$DIR"
# Never leave a publishable old feed after a failed signing/build attempt.
rm -f "$DIR/$ARCHIVE" "$DIR/$ARCHIVE.sig" "$DIR/$FEED" "$DIR/$FEED.sig" "$DIR/$FEED2" "$DIR/$FEED2.sig" "$DIR/latest.json" "$DIR/native-release.json"
KEY="${WINGS_UPDATER_KEY:-$HOME/.tauri/wings-updater.key}"
[ -f "$KEY" ] || { echo "No updater key at $KEY. Set WINGS_UPDATER_KEY to the native updater signing key." >&2; exit 1; }
KEY="$(cd "$(dirname "$KEY")" && pwd)/$(basename "$KEY")"
sh scripts/build-mac-app.sh
COPYFILE_DISABLE=1 tar -czf "$DIR/$ARCHIVE" -C target/release "$BUNDLE"
SHA256=$(shasum -a 256 "$DIR/$ARCHIVE" | awk '{print $1}')
SIZE=$(wc -c < "$DIR/$ARCHIVE" | tr -d ' ')
NOTES="${1-}"
export VERSION PLATFORM ARCHIVE SHA256 SIZE NOTES IDENTIFIER TAG SERVER_VERSION
# Sign temporary files, then expose each feed only after both its files exist.
TMP="$DIR/$FEED.pending"
TMP2="$DIR/$FEED2.pending"
trap 'rm -f "$TMP" "$TMP.sig" "$TMP2" "$TMP2.sig"' EXIT HUP INT TERM
node -e 'const {VERSION,PLATFORM,ARCHIVE,SHA256,SIZE,NOTES,IDENTIFIER,SERVER_VERSION}=process.env; const feed={bundle_identifier:IDENTIFIER,version:VERSION,platform:PLATFORM,artifact:ARCHIVE,size:Number(SIZE),sha256:SHA256,notes:NOTES}; const fs=require("fs"); fs.writeFileSync(process.argv[1], JSON.stringify(feed)+"\n"); fs.writeFileSync(process.argv[2], JSON.stringify({...feed,server_version:Number(SERVER_VERSION)})+"\n")' "$TMP" "$TMP2"
for pending in "$TMP" "$TMP2"; do
  (cd ../desktop && pnpm tauri signer sign --private-key-path "$KEY" --password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD-}" "../native/$pending")
  [ -s "$pending.sig" ] || { echo "Signing did not produce the native feed signature" >&2; exit 1; }
done
mv "$TMP.sig" "$DIR/$FEED.sig"
mv "$TMP" "$DIR/$FEED"
mv "$TMP2.sig" "$DIR/$FEED2.sig"
mv "$TMP2" "$DIR/$FEED2"
echo "Packaged $DIR/$ARCHIVE and signed $FEED and $FEED2"
if [ "${WINGS_SWITCH-}" = 1 ]; then
  # The desktop's updater (Tauri's) checks latest.json, verifies the archive's own signature, and swaps the bundle in its
  # tar for the running Wings.app, then starts the new bundle's CFBundleExecutable.
  (cd ../desktop && pnpm tauri signer sign --private-key-path "$KEY" --password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD-}" "../native/$DIR/$ARCHIVE")
  [ -s "$DIR/$ARCHIVE.sig" ] || { echo "Signing did not produce the archive signature" >&2; exit 1; }
  node -e 'const fs=require("fs"); const {VERSION,PLATFORM,ARCHIVE,NOTES,TAG}=process.env; fs.writeFileSync(process.argv[1], JSON.stringify({version:VERSION,notes:NOTES,pub_date:new Date().toISOString(),platforms:{[PLATFORM]:{signature:fs.readFileSync(process.argv[2],"utf8").trim(),url:`https://github.com/josippapez/Wings/releases/download/${TAG}/${ARCHIVE}`}}},null,2)+"\n")' "$DIR/latest.json" "$DIR/$ARCHIVE.sig"
  echo "Publish $ARCHIVE, $ARCHIVE.sig, $FEED, $FEED.sig, $FEED2, $FEED2.sig and latest.json in a GitHub release tagged $TAG in josippapez/Wings, and make it the latest release."
else
  echo "Publish all five files ($ARCHIVE, $FEED, $FEED.sig, $FEED2, $FEED2.sig) in a GitHub release tagged $TAG in josippapez/Wings. Include both architecture feeds in the latest release when shipping both."
fi
