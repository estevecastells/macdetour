#!/bin/bash
# Install or update the MacDetour daemon. Run as root (scripts/setup.sh does it for you).
#
#   sudo scripts/install.sh --bin-dir target/release --host 203.0.113.10 --user tunnel [--port 22] \
#        --known-hosts /Users/me/.ssh/known_hosts --allow-uid 501
set -euo pipefail

label="io.github.estevecastells.macdetour"
support="/Library/Application Support/MacDetour"
libexec="/usr/local/libexec/macdetour"
plist="/Library/LaunchDaemons/${label}.plist"
socket="/var/run/macdetour.sock"

bin_dir="" host="" user="" port="22" known_hosts="" allow_uid=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin-dir) bin_dir="$2"; shift 2 ;;
    --host) host="$2"; shift 2 ;;
    --user) user="$2"; shift 2 ;;
    --port) port="$2"; shift 2 ;;
    --known-hosts) known_hosts="$2"; shift 2 ;;
    --allow-uid) allow_uid="$2"; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done

[[ $EUID -eq 0 ]] || { echo "install.sh must run as root" >&2; exit 1; }
[[ -x "$bin_dir/detourd" && -x "$bin_dir/detourctl" ]] || { echo "missing binaries in $bin_dir" >&2; exit 1; }
[[ "$host" =~ ^[0-9A-Fa-f:.]+$ ]] || { echo "--host must be an IP address" >&2; exit 1; }
[[ "$user" =~ ^[A-Za-z0-9._-]+$ && "$user" != -* ]] || { echo "--user is invalid" >&2; exit 1; }
[[ "$port" =~ ^[0-9]+$ ]] || { echo "--port is invalid" >&2; exit 1; }
[[ "$allow_uid" =~ ^[0-9]+$ ]] || { echo "--allow-uid is required" >&2; exit 1; }

echo "==> Installing binaries"
install -d -m 755 "$libexec" /usr/local/bin
install -m 755 "$bin_dir/detourd" "$libexec/detourd"
install -m 755 "$bin_dir/detourctl" /usr/local/bin/detourctl

echo "==> Preparing $support"
install -d -m 755 "$support"
if [[ ! -f "$support/id_ed25519" ]]; then
  ssh-keygen -q -t ed25519 -N "" -C "macdetour@$(scutil --get LocalHostName 2>/dev/null || hostname -s)" \
    -f "$support/id_ed25519"
fi
chmod 600 "$support/id_ed25519"
chmod 644 "$support/id_ed25519.pub"

# Pin the server key from the user's own known_hosts (never trust on first use here).
lookup="$host"
[[ "$port" != "22" ]] && lookup="[$host]:$port"
pinned="$(ssh-keygen -F "$lookup" -f "$known_hosts" 2>/dev/null | grep -v '^#' || true)"
[[ -n "$pinned" ]] || { echo "no known_hosts entry for $lookup in $known_hosts; ssh to it once first" >&2; exit 1; }
printf '%s\n' "$pinned" > "$support/known_hosts"
chmod 644 "$support/known_hosts"

if [[ ! -f "$support/config.toml" ]]; then
  cat > "$support/config.toml" <<TOML
# MacDetour daemon configuration. Restart after editing:
#   sudo launchctl kickstart -k system/${label}
ssh_host = "${host}"
ssh_port = ${port}
ssh_user = "${user}"
allowed_uids = [${allow_uid}]
# Destinations always detoured while on, e.g. ["188.114.96.0/20"].
extra_routes = []
TOML
fi
chmod 644 "$support/config.toml"

echo "==> Registering the launchd service"
cat > "$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>${label}</string>
  <key>ProgramArguments</key>
  <array><string>${libexec}/detourd</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardErrorPath</key><string>/var/log/macdetour.log</string>
  <key>StandardOutPath</key><string>/var/log/macdetour.log</string>
</dict>
</plist>
PLIST
chmod 644 "$plist"
launchctl bootout "system/${label}" 2>/dev/null || true
launchctl bootstrap system "$plist"

for _ in $(seq 1 20); do
  [[ -S "$socket" ]] && break
  sleep 0.5
done
[[ -S "$socket" ]] || { echo "detourd did not start; see /var/log/macdetour.log" >&2; exit 1; }
echo "==> detourd is running"
