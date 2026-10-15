#!/bin/sh
# Packs each folder in the repo's plugins/ into <out-dir>/<id>.wings-plugin with wings-plugin pack: a zip with
# wings-plugin.json at its root and the built files in dist/, without the plugin's source. Prints the path of each
# file. release-mac.sh attaches them to the release, so github.com/josippapez/Wings/plugins/<id> installs and
# updates from there.
#   sh scripts/pack-plugins.sh <out-dir>
set -e
OUT="${1:?Usage: pack-plugins.sh <out-dir>}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
for manifest in "$ROOT"/plugins/*/wings-plugin.json; do
  dir=$(dirname "$manifest")
  id=$(basename "$dir")
  # pack writes <id>.wings-plugin into the plugin folder it runs in.
  (cd "$dir" && node "$ROOT/packages/plugin-sdk/bin.mjs" pack >/dev/null)
  mv "$dir/$id.wings-plugin" "$OUT/$id.wings-plugin"
  echo "$OUT/$id.wings-plugin"
done
