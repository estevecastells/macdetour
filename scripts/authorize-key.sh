#!/bin/bash
# Authorize MacDetour's tunnel key on the server, restricted to port forwarding:
# no shell, no commands, no agent or X11 forwarding. Run as your user; it uses
# your own SSH access to the server once.
#
#   scripts/authorize-key.sh <ssh-alias>
set -euo pipefail
alias_name="${1:?usage: authorize-key.sh <ssh-alias>}"
pub="$(cat "/Library/Application Support/MacDetour/id_ed25519.pub")"
key_body="$(cut -d' ' -f2 <<<"$pub")"
[[ "$key_body" =~ ^[A-Za-z0-9+/=]+$ ]] || { echo "unexpected public key format" >&2; exit 1; }
entry="restrict,port-forwarding,command=\"/usr/bin/false\" ${pub}"
ssh "$alias_name" "umask 077 && mkdir -p ~/.ssh && touch ~/.ssh/authorized_keys && \
  (grep -qF '${key_body}' ~/.ssh/authorized_keys || printf '%s\n' '${entry}' >> ~/.ssh/authorized_keys)"
