#!/bin/sh
# Builds a signed Wings update and writes latest.json for it. Publishing is left to you: this prints the
# `gh release create` command. See docs/releasing.md.
#   pnpm release:mac            release notes: this version's section of CHANGELOG.md
#   pnpm release:mac "notes"    release notes: your text
set -e
cd "$(dirname "$0")/.."

KEY="${WINGS_UPDATER_KEY:-$HOME/.tauri/wings-updater.key}"
[ -f "$KEY" ] || { echo "No updater key at $KEY. Make one with: pnpm tauri signer generate -w $KEY" >&2; exit 1; }

VERSION=$(node -p 'require("./src-tauri/tauri.conf.json").version')
if [ -n "$1" ]; then NOTES="$1"; else NOTES=$(node scripts/release-notes.mjs "$VERSION"); fi
case "$(uname -m)" in
  arm64) ARCH=aarch64 ;;
  *) ARCH=x86_64 ;;
esac
REPO=josippapez/Wings

# Checks before the build, which takes minutes: a release is what's committed, its tests pass, and each plugin's
# committed built files match its source (`pnpm check` compares the content of each built file).
[ -z "$(git status --porcelain)" ] || { echo "Commit or stash your changes first: a release builds what's in the tree." >&2; git status --short >&2; exit 1; }
pnpm test
(cd src-tauri && cargo test --lib --quiet)
(cd ../../packages/plugin-sdk && pnpm install --frozen-lockfile >/dev/null)
(cd ../../packages/plugin-ui && pnpm install --frozen-lockfile >/dev/null)
(cd ../../plugins && pnpm install --frozen-lockfile >/dev/null && pnpm check)

# Read the key into the build's environment only. Tauri signs the update archive with it; it is never written elsewhere.
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD-}"

DIR="src-tauri/target/release/bundle/macos"
ARCHIVE="$DIR/Wings.app.tar.gz"
# The last release's files go first: after a failed build, `gh release create` finds them missing and publishes nothing,
# instead of uploading the old version under the new tag.
rm -f "$ARCHIVE" "$ARCHIVE.sig" "$DIR/latest.json" "$DIR/notes.md" "$DIR"/*.wings-plugin
sh scripts/build-mac-app.sh

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

# Each official plugin as <id>.wings-plugin, for installing from a file. A github.com/josippapez/Wings/plugins/<id> link
# installs that folder from the default branch instead (plugin_store.rs, pick).
PACKED=$(sh scripts/pack-plugins.sh "$DIR")
PLUGINS=$(printf '%s\n' "$PACKED" | while read -r f; do printf ' "%s"' "$f"; done)

# The debug build keeps every old copy of the app's own code and grew to 29 GB between two releases. The release
# build holds the files to publish, so the printed command cleans it once they are uploaded.
(cd src-tauri && cargo clean --profile dev)

echo
echo "Built Wings $VERSION. Publish it with:"
echo
# Notes go in a file: pasted inside quotes, a backtick in them would run as a command.
printf '%s\n' "$NOTES" > "$DIR/notes.md"
echo "  gh release create v$VERSION \"$PWD/$ARCHIVE\" \"$PWD/$ARCHIVE.sig\" \"$PWD/$DIR/latest.json\"$PLUGINS --repo $REPO --target main --title \"Wings $VERSION\" --notes-file \"$PWD/$DIR/notes.md\" && (cd \"$PWD/src-tauri\" && cargo clean --release)"
