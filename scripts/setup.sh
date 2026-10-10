#!/bin/bash
# One-shot setup, run as your normal user (it asks for sudo once):
#   scripts/setup.sh <ssh-alias>        e.g. scripts/setup.sh my-server
#
# 1. Builds the daemon, CLI and menu bar app.
# 2. Installs the daemon as root (creates a dedicated tunnel key).
# 3. Authorizes that key on the server with your own SSH access, restricted to
#    port forwarding: no shell, no commands, no agent or X11 forwarding.
# 4. Installs MacDetour.app in /Applications and opens it.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
alias_name="${1:?usage: scripts/setup.sh <ssh-alias>}"
[[ $EUID -ne 0 ]] || { echo "run setup.sh as your user, not root" >&2; exit 1; }

read -r host_ip ssh_user ssh_port < <(scripts/resolve-host.sh "$alias_name")
echo "==> Tunnel server: ${ssh_user}@${host_ip}:${ssh_port} (from alias ${alias_name})"

echo "==> Building"
cargo build --release --locked -p detourd -p detourctl
scripts/build-app.sh

echo "==> Installing the daemon (sudo)"
sudo scripts/install.sh --bin-dir target/release --host "$host_ip" --user "$ssh_user" --port "$ssh_port" \
  --known-hosts "$HOME/.ssh/known_hosts" --allow-uid "$(id -u)"

echo "==> Authorizing the tunnel key on ${alias_name} (forwarding only)"
scripts/authorize-key.sh "$alias_name"

echo "==> Installing MacDetour.app"
rm -rf /Applications/MacDetour.app
cp -R build/MacDetour.app /Applications/
open /Applications/MacDetour.app
echo
echo "Done. Switch it on from the menu bar, or run: detourctl on"
