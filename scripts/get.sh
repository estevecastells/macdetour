#!/bin/bash
# Install or upgrade MacDetour from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/estevecastells/macdetour/main/scripts/get.sh | bash -s -- <ssh-alias>
#
# <ssh-alias> is a host from your ~/.ssh/config that you can already reach.
# Environment options:
#   MACDETOUR_VERSION=v0.1.0   install a specific release (default: latest)
#   MACDETOUR_DOWNLOAD_ONLY=1  download and verify into a temp dir, then stop
#
# Downloads with curl don't get macOS's quarantine flag, so the unsigned app
# opens without a Gatekeeper prompt. Checksums are verified before anything runs.
set -euo pipefail

REPO="estevecastells/macdetour"
APP_ZIP="MacDetour-macos-arm64.zip"
CLI_TGZ="macdetour-macos-arm64.tar.gz"

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

alias_name="${1:-}"
[[ -n "$alias_name" || -n "${MACDETOUR_DOWNLOAD_ONLY:-}" ]] || die "usage: get.sh <ssh-alias>"
[[ "$(uname -s)" == "Darwin" ]] || die "MacDetour only runs on macOS"
[[ "$(uname -m)" == "arm64" ]] || die "MacDetour requires an Apple Silicon Mac"
major="$(sw_vers -productVersion | cut -d. -f1)"
(( major >= 14 )) || die "macOS 14 or later is required"
[[ $EUID -ne 0 ]] || die "run as your user, not root (it asks for sudo once)"

version="${MACDETOUR_VERSION:-latest}"
if [[ "$version" == "latest" ]]; then
  base="https://github.com/${REPO}/releases/latest/download"
else
  base="https://github.com/${REPO}/releases/download/${version}"
fi

tmp="$(mktemp -d)"
if [[ -z "${MACDETOUR_DOWNLOAD_ONLY:-}" ]]; then
  trap 'rm -rf "$tmp"' EXIT
fi

say "Downloading MacDetour (${version})"
for f in SHA256SUMS "$CLI_TGZ" "$APP_ZIP"; do
  curl -fSL --progress-bar -o "${tmp}/${f}" "${base}/${f}" || die "download failed: ${base}/${f}"
done

say "Verifying checksums"
(cd "$tmp" && shasum -a 256 -c SHA256SUMS) || die "checksum mismatch; aborting"
tar -xzf "${tmp}/${CLI_TGZ}" -C "$tmp"
pkg="${tmp}/macdetour"

if [[ -n "${MACDETOUR_DOWNLOAD_ONLY:-}" ]]; then
  say "Downloaded and verified in ${tmp}"
  exit 0
fi

read -r host_ip ssh_user ssh_port < <(/bin/bash "${pkg}/resolve-host.sh" "$alias_name") \
  || die "cannot resolve ${alias_name} from your ssh config"
say "Tunnel server: ${ssh_user}@${host_ip}:${ssh_port} (from ${alias_name})"
ssh-keygen -F "$host_ip" -f "$HOME/.ssh/known_hosts" >/dev/null 2>&1 \
  || ssh-keygen -F "[$host_ip]:$ssh_port" -f "$HOME/.ssh/known_hosts" >/dev/null 2>&1 \
  || die "run 'ssh ${alias_name}' once first so its host key is in ~/.ssh/known_hosts"

say "Installing the detour service (requires your password)"
sudo /bin/bash "${pkg}/install.sh" --bin-dir "$pkg" --host "$host_ip" --user "$ssh_user" --port "$ssh_port" \
  --known-hosts "$HOME/.ssh/known_hosts" --allow-uid "$(id -u)"

say "Authorizing the tunnel key on ${alias_name} (port forwarding only)"
/bin/bash "${pkg}/authorize-key.sh" "$alias_name"

dest="/Applications"
[[ -w "$dest" ]] || dest="${HOME}/Applications"
mkdir -p "$dest"
say "Installing the menu bar app to ${dest}"
pkill -x MacDetour 2>/dev/null || true
rm -rf "${dest}/MacDetour.app"
ditto -x -k "${tmp}/${APP_ZIP}" "$dest"
open "${dest}/MacDetour.app"

say "Done. Switch it on from the menu bar, or run: detourctl on"
