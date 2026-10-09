#!/bin/sh
# Builds Wings.app, signed with a "Wings Local Signing" certificate from your keychain when there is one.
# macOS keys privacy and keychain approvals to the signature, and an ad-hoc signature changes with every build,
# so without a fixed certificate each build asks again. See docs/signing.md to make the certificate.
set -e
if security find-identity -p codesigning | grep -q '"Wings Local Signing"'; then
  export APPLE_SIGNING_IDENTITY="Wings Local Signing"
fi
exec pnpm tauri build --bundles app "$@"
