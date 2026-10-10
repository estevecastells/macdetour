# Security policy

`detourd` runs as root, changes routes and holds an SSH key for your server, so we take security reports seriously.

## Reporting

Please report vulnerabilities privately via [GitHub security advisories](https://github.com/estevecastells/macdetour/security/advisories/new), not public issues.

## Threat model (summary)

- Any local user can connect to `/var/run/macdetour.sock` and read status. Switching the detour on or off or forcing a refresh requires the connecting process's uid (checked with the peer credentials) to be root or listed in `allowed_uids`.
- Requests are size-limited JSON lines. Malformed input gets an error response and never crashes the daemon (see `crates/detourd/tests/socket.rs`).
- Routes are only added for single addresses or narrow networks from the list; local, private, link-local, multicast and reserved ranges, broad networks and the tunnel server itself are never routed (see `detour-core`).
- The tunnel key is generated on your Mac, readable only by root, and authorized on the server as `restrict,port-forwarding,command="/usr/bin/false"`: it can forward ports but cannot open a shell or run commands. The server's host key is pinned from your own `known_hosts`; the daemon never trusts on first use.
- The daemon runs `ssh`, `route`, `ifconfig` and `curl` with fixed absolute paths and argument lists, never through a shell.
- The in-app updater installs nothing unless the release's `SHA256SUMS` carries a valid Ed25519 signature from the release key and the download matches it.
