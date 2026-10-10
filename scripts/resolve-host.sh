#!/bin/bash
# Print "<ip> <user> <port>" for an SSH alias from your ~/.ssh/config.
#   scripts/resolve-host.sh <ssh-alias>
set -euo pipefail
resolved="$(ssh -G "${1:?usage: resolve-host.sh <ssh-alias>}")"
host_name="$(awk '$1=="hostname"{print $2}' <<<"$resolved")"
user="$(awk '$1=="user"{print $2}' <<<"$resolved")"
port="$(awk '$1=="port"{print $2}' <<<"$resolved")"
ip="$host_name"
if ! [[ "$ip" =~ ^[0-9A-Fa-f:.]+$ ]]; then
  ip="$(dscacheutil -q host -a name "$host_name" | awk '/ip_address/{print $2; exit}')"
fi
[[ -n "$ip" ]] || { echo "cannot resolve $host_name" >&2; exit 1; }
echo "$ip $user $port"
