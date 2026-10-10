<div align="center">

<h1>MacDetour</h1>

<p><strong>Free, open-source menu bar switch for macOS that sends only blocked addresses through your own server.</strong><br/>
Made for LaLiga's IP blocks in Spain: everything else keeps using your normal connection, untouched.</p>

<a href="https://github.com/estevecastells/macdetour/releases/latest/download/MacDetour-macos-arm64.zip"><img src="https://img.shields.io/badge/Download_for_Mac-Apple_Silicon-0A84FF?style=for-the-badge&logo=apple&logoColor=white" alt="Download MacDetour for Mac (Apple Silicon)" height="44"/></a>
&nbsp;
<a href="https://github.com/estevecastells/macdetour/releases/latest/download/macdetour-macos-arm64.tar.gz"><img src="https://img.shields.io/badge/Command_line-.tar.gz-24292F?style=for-the-badge&logo=gnubash&logoColor=white" alt="Download the command-line tools" height="44"/></a>

<p>
<a href="https://github.com/estevecastells/macdetour/releases/latest"><img src="https://img.shields.io/github/v/release/estevecastells/macdetour?label=latest&style=flat-square" alt="Latest release"/></a>
<a href="https://github.com/estevecastells/macdetour/releases"><img src="https://img.shields.io/github/downloads/estevecastells/macdetour/total?style=flat-square" alt="Downloads"/></a>
<img src="https://img.shields.io/badge/macOS-14%2B-555?style=flat-square&logo=apple" alt="macOS 14 or later"/>
<a href="https://github.com/estevecastells/macdetour/actions/workflows/ci.yml"><img src="https://github.com/estevecastells/macdetour/actions/workflows/ci.yml/badge.svg" alt="CI"/></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT license"/></a>
</p>

**Install in one line** (no security prompt; verifies checksums):

</div>

```sh
curl -fsSL https://raw.githubusercontent.com/estevecastells/macdetour/main/scripts/get.sh | bash -s -- <your-ssh-alias>
```

<div align="center"><sub>
Needs a server you already reach with <code>ssh &lt;alias&gt;</code>. The installer asks for your password once, sets up the service,
and authorizes a forwarding-only key on that server. The app alone does nothing without the service, so prefer the one-liner.
Requires an Apple Silicon Mac on macOS 14+.
</sub></div>

<br/>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/panel-dark.png">
    <img src="docs/images/panel-light.png" alt="MacDetour menu bar panel: tunnel connected, 278 blocked addresses detoured" width="300">
  </picture>
</p>

---

## How it works

```
apps ──▶ macOS routing ──┬─ everything else ─────────────▶ your network
                         └─ blocked IPs only ─▶ utun ─▶ tun2proxy ─▶ ssh -D ─▶ your server ─▶ internet
```

- **What gets detoured.** The daemon downloads the public list of currently
  blocked addresses ([hayahora.futbol](https://hayahora.futbol)) every 5 minutes
  and probes each one directly on TCP 443, through your physical interface. Only
  addresses that do **not** answer directly get a host route into the tunnel. On
  a network without blocks, nothing is detoured.
- **No interruptions.** Connections that work are never touched, because their
  addresses are never routed. Routes are only added while the detour is on,
  never removed, so a refresh cannot cut a connection that is flowing through
  the tunnel. Switching off removes every route at once.
- **Every app is covered**, including command-line tools: it works at the
  routing layer, not with proxy settings.
- **Network changes.** A Wi-Fi or hotspot switch restarts the SSH session at
  once and re-probes the list on the new network.
- **UDP/QUIC** to detoured addresses is not carried (SSH dynamic forwarding is
  TCP only). Browsers fall back to TCP automatically.

## Pieces

| Part | What it does |
|---|---|
| `crates/detour-core` | Config, blocklist parsing, route planning, wire protocol. No OS calls. |
| `crates/detourd` | Root daemon (launchd): SSH supervisor, utun + [tun2proxy](https://github.com/tun2proxy/tun2proxy), routes, probes, control socket `/var/run/macdetour.sock`. |
| `crates/detourctl` | CLI: `detourctl status`, `on`, `off`, `refresh`. |
| `app/` | SwiftUI menu bar app with the on/off switch. |

## Install

**One line (recommended).** Downloads the latest release, verifies checksums, installs the
service, authorizes the tunnel key on your server and opens the app:

```bash
curl -fsSL https://raw.githubusercontent.com/estevecastells/macdetour/main/scripts/get.sh | bash -s -- <your-ssh-alias>
```

**From source.**

```bash
make setup SSH=<your-ssh-alias>      # e.g. make setup SSH=my-server
```

Both ask for `sudo` once to install the daemon, then use your own SSH access to authorize a
dedicated tunnel key on the server. That key is restricted to port forwarding
(`restrict,port-forwarding,command="/usr/bin/false"`): it cannot open a shell or run commands.
The daemon never uses your personal key.

The server needs `AllowTcpForwarding yes` (the OpenSSH default), and its host key must already be
in your `~/.ssh/known_hosts` (connect once with `ssh <alias>`).

## Use

Click the menu bar icon and flip the switch, or:

```bash
detourctl on
detourctl status
detourctl off
```

`make probe` shows which listed addresses are blocked on your current network
without changing anything.

Config: `/Library/Application Support/MacDetour/config.toml`. Log: `/var/log/macdetour.log`.

## Updates

The app checks GitHub for a new release every six hours (or on **Buscar actualizaciones**). It installs
nothing unless the release's checksums carry a valid Ed25519 signature from the release key and the
download matches them; then it replaces itself and relaunches, and offers to update the service, which asks
for your password. Turn off **Instalar actualizaciones automáticamente** to only be notified. See
[docs/RELEASING.md](docs/RELEASING.md).

## Uninstall

```bash
make uninstall                     # keeps the key and config
sudo scripts/uninstall.sh --purge  # also deletes them and prints the key to remove from the server
```

## Screenshots

`cd app && swift run RenderScreenshots ../docs/images` renders the panel (light
and dark) and a 1600×900 social card from sample data with the app's real views.

## Tests

```bash
make ci
```

The engine is tested against a fake platform (routing, sticky routes, network
changes, failures, restore after restart); the socket against a fake engine;
the Swift models against the same JSON fixtures as Rust.
