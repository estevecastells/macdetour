#!/bin/bash
# Build release artifacts into dist/:
#   MacDetour-macos-arm64.zip     menu bar app
#   macdetour-macos-arm64.tar.gz  detourd, detourctl and the install scripts
#   SHA256SUMS
#   SHA256SUMS.sig                Ed25519 signature the in-app updater checks (needs $UPDATE_SIGNING_KEY)
#   RELEASE_NOTES.md              install instructions prepended to the release notes
#
# Asset names carry no version so that .../releases/latest/download/<name> always works.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

export MACOSX_DEPLOYMENT_TARGET=14.0
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
repo="estevecastells/macdetour"

scripts/build-app.sh

rm -rf dist
stage="dist/stage/macdetour"
mkdir -p "$stage"
cp target/release/detourd target/release/detourctl \
  scripts/install.sh scripts/uninstall.sh scripts/authorize-key.sh scripts/resolve-host.sh LICENSE "$stage/"
codesign --force --sign - "$stage/detourd" "$stage/detourctl" >/dev/null
tar -C dist/stage -czf dist/macdetour-macos-arm64.tar.gz macdetour
ditto -c -k --sequesterRsrc --keepParent build/MacDetour.app dist/MacDetour-macos-arm64.zip
rm -rf dist/stage

(cd dist && shasum -a 256 MacDetour-macos-arm64.zip macdetour-macos-arm64.tar.gz > SHA256SUMS)
if [[ -n "${UPDATE_SIGNING_KEY:-}" ]]; then
  swift scripts/sign-update.swift dist/SHA256SUMS > dist/SHA256SUMS.sig
else
  echo "warning: UPDATE_SIGNING_KEY not set; release won't be installable by the in-app updater" >&2
fi

cat > dist/RELEASE_NOTES.md <<NOTES
## Install

You need a server you can already reach with \`ssh <alias>\` (an entry in your \`~/.ssh/config\`).
This installs the detour service, authorizes a forwarding-only key on that server, installs the
menu bar app and verifies checksums:

\`\`\`sh
curl -fsSL https://raw.githubusercontent.com/${repo}/main/scripts/get.sh | bash -s -- <your-ssh-alias>
\`\`\`

The app on its own does nothing without the service, so use the command above rather than the zip.
Requires an Apple Silicon Mac on macOS 14 or later.

NOTES

echo "==> Release artifacts (v${version}):"
ls -la dist
cat dist/SHA256SUMS
