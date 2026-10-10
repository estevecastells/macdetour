#!/bin/bash
# Remove the MacDetour daemon. Switching it off first removes every detour route.
#   sudo scripts/uninstall.sh [--purge]   (--purge also deletes the tunnel key and config)
set -euo pipefail
label="io.github.estevecastells.macdetour"
support="/Library/Application Support/MacDetour"
[[ $EUID -eq 0 ]] || { echo "uninstall.sh must run as root" >&2; exit 1; }

/usr/local/bin/detourctl off 2>/dev/null || true
launchctl bootout "system/${label}" 2>/dev/null || true
rm -f "/Library/LaunchDaemons/${label}.plist" /usr/local/bin/detourctl /var/run/macdetour.sock
rm -rf /usr/local/libexec/macdetour
if [[ "${1:-}" == "--purge" ]]; then
  [[ -f "$support/id_ed25519.pub" ]] && echo "Remove this key from the server's ~/.ssh/authorized_keys: $(cut -d' ' -f2 "$support/id_ed25519.pub")"
  rm -rf "$support"
fi
echo "MacDetour removed."
