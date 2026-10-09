#!/bin/sh
# Builds a signed Wings update and writes latest.json for it. Publishing is left to you: this prints the
# `gh release create` command. See docs/releasing.md.
#   pnpm release:mac            release notes: "Wings X.Y.Z"
#   pnpm release:mac "notes"    release notes: your text
set -e
cd "$(dirname "$0")/.."

KEY="${WINGS_UPDATER_KEY:-$HOME/.tauri/wings-updater.key}"
[ -f "$KEY" ] || { echo "No updater key at $KEY. Make one with: pnpm tauri signer generate -w $KEY" >&2; exit 1; }

VERSION=$(node -p 'require("./src-tauri/tauri.conf.json").version')
NOTES="${1:-Wings $VERSION}"
case "$(uname -m)" in
  arm64) ARCH=aarch64 ;;
  *) ARCH=x86_64 ;;
esac
REPO=josippapez/Wings

# Read the key into the build's environment only. Tauri signs the update archive with it; it is never written elsewhere.
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD-}"
sh scripts/build-mac-app.sh

DIR="src-tauri/target/release/bundle/macos"
ARCHIVE="$DIR/Wings.app.tar.gz"
[ -f "$ARCHIVE" ] && [ -f "$ARCHIVE.sig" ] || { echo "Build didn't produce $ARCHIVE and its .sig" >&2; exit 1; }

# The updater fetches this file from the latest release, so it names the assets of the release being published.
export VERSION NOTES ARCH REPO ARCHIVE
node -e '
const fs = require("fs");
const { VERSION, NOTES, ARCH, REPO, ARCHIVE } = process.env;
const latest = {
  version: VERSION,
  notes: NOTES,
  pub_date: new Date().toISOString(),
  platforms: {
    [`darwin-${ARCH}`]: {
      signature: fs.readFileSync(ARCHIVE + ".sig", "utf8").trim(),
      url: `https://github.com/${REPO}/releases/download/v${VERSION}/Wings.app.tar.gz`,
    },
  },
};
fs.writeFileSync(process.argv[1], JSON.stringify(latest, null, 2) + "\n");
' "$DIR/latest.json"

echo
echo "Built Wings $VERSION. Publish it with:"
echo
echo "  gh release create v$VERSION \"$PWD/$ARCHIVE\" \"$PWD/$ARCHIVE.sig\" \"$PWD/$DIR/latest.json\" --repo $REPO --title \"Wings $VERSION\" --notes \"$NOTES\""
