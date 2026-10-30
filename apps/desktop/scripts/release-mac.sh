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
# The builds keep the dependencies from release to release, and only the app's own crate is compiled again (see the
# clean below). Dependencies change only with Cargo.lock, so when it changed for more than this app's version, start
# from a clean build and the old dependency versions don't pile up.
LOCK=$(sed '/^name = "wings"$/,/^$/d' src-tauri/Cargo.lock | shasum | cut -d' ' -f1)
[ "$(cat src-tauri/target/release-lock 2>/dev/null)" = "$LOCK" ] || (cd src-tauri && cargo clean)
pnpm test
(cd src-tauri && cargo test --lib --quiet)
(cd ../../plugins && pnpm run deps >/dev/null && pnpm check)
# Installed Wings takes a plugin's update only when its version goes up, so a plugin changed since the last release
# without a bump would never reach anyone. Its preview/ is only for screenshots, so a change there needs no bump.
LAST=$(git describe --tags --abbrev=0 --match 'v*' 2>/dev/null || true)
if [ -n "$LAST" ]; then
  stale=""
  for m in ../../plugins/*/wings-plugin.json; do
    d=${m%/wings-plugin.json}
    git diff --quiet "$LAST" HEAD -- "$d" ":(exclude)$d/preview" && continue
    was=$(git show "$LAST:${d#../../}/wings-plugin.json" 2>/dev/null | node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>{try{console.log(JSON.parse(s).version)}catch{}})')
    [ -n "$was" ] && [ "$was" = "$(node -p "require('$m').version")" ] && stale="$stale ${d#../../}"
  done
  [ -z "$stale" ] || { echo "Changed since $LAST without a version bump:$stale" >&2; exit 1; }
fi

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
echo "$LOCK" > src-tauri/target/release-lock

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

# Each version compiles a new copy of the app's own crate, and the debug build grew to 29 GB of them between two
# releases. `--workspace` drops every copy of it and keeps the dependencies. The release build holds the files to
# publish, so the printed command does the same there once they are uploaded.
(cd src-tauri && cargo clean --profile dev --workspace)

echo
echo "Built Wings $VERSION. Publish it with:"
echo
# Notes go in a file: pasted inside quotes, a backtick in them would run as a command.
printf '%s\n' "$NOTES" > "$DIR/notes.md"
echo "  gh release create v$VERSION \"$PWD/$ARCHIVE\" \"$PWD/$ARCHIVE.sig\" \"$PWD/$DIR/latest.json\"$PLUGINS --repo $REPO --target main --title \"Wings $VERSION\" --notes-file \"$PWD/$DIR/notes.md\" && (cd \"$PWD/src-tauri\" && cargo clean --release --workspace)"
