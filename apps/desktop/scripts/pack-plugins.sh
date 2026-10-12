#!/bin/sh
# Packs each folder in the repo's plugins/ into <out-dir>/<id>.wings-plugin, a zip with wings-plugin.json at its
# root, from the committed files. Prints the path of each file. release-mac.sh attaches them to the release, so
# github.com/josippapez/Wings/plugins/<id> installs and updates from there.
#   sh scripts/pack-plugins.sh <out-dir>
set -e
OUT="${1:?Usage: pack-plugins.sh <out-dir>}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
for manifest in "$ROOT"/plugins/*/wings-plugin.json; do
  id=$(basename "$(dirname "$manifest")")
  git -C "$ROOT" archive --format=zip --output "$OUT/$id.wings-plugin" "HEAD:plugins/$id"
  echo "$OUT/$id.wings-plugin"
done
