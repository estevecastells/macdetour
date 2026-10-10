//! Thin wrappers over macOS networking commands. Argument building and output
//! parsing are pure functions so they can be tested without root.

use detour_core::Cidr;
use std::process::Command;

pub const ROUTE: &str = "/sbin/route";
pub const IFCONFIG: &str = "/sbin/ifconfig";

/// Address given to the utun interface so IPv6 routes can point at it.
/// Unique-local, never announced anywhere.
pub const TUN_IPV6: &str = "fdde:7e70:ee00::1";

/// The interface and gateway that carry direct traffic.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DefaultRoute {
    pub interface: Option<String>,
    pub gateway: Option<String>,
}

/// Parse `route -n get default` output.
pub fn parse_route_get(output: &str) -> DefaultRoute {
    let mut route = DefaultRoute::default();
    for line in output.lines() {
        let Some((key, value)) = line.trim().split_once(':') else { continue };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "interface" => route.interface = Some(value.to_string()),
            "gateway" => route.gateway = Some(value.to_string()),
            _ => {}
        }
    }
    route
}

pub fn default_route(ipv6: bool) -> DefaultRoute {
    let family = if ipv6 { "-inet6" } else { "-inet" };
    match Command::new(ROUTE).args(["-n", "get", family, "default"]).output() {
        Ok(out) if out.status.success() => parse_route_get(&String::from_utf8_lossy(&out.stdout)),
        _ => DefaultRoute::default(),
    }
}

/// Arguments for `route` to add or delete a detour route through `interface`.
pub fn route_args(action: &str, dest: &Cidr, interface: &str) -> Vec<String> {
    let family = if dest.is_ipv4() { "-inet" } else { "-inet6" };
    let kind = if dest.is_host() { "-host" } else { "-net" };
    vec![
        "-q".into(),
        "-n".into(),
        action.into(),
        family.into(),
        kind.into(),
        dest.to_string(),
        "-interface".into(),
        interface.into(),
    ]
}

fn run(program: &str, args: &[String]) -> Result<(), String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if msg.is_empty() { format!("{program} exited with {}", out.status) } else { msg })
    }
}

/// Route `dest` through `interface`, replacing any stale route to it.
pub fn add_route(dest: &Cidr, interface: &str) -> Result<(), String> {
    match run(ROUTE, &route_args("add", dest, interface)) {
        Ok(()) => Ok(()),
        Err(e) if e.contains("File exists") => {
            let _ = delete_route_any(dest);
            run(ROUTE, &route_args("add", dest, interface))
        }
        Err(e) => Err(e),
    }
}

pub fn delete_route(dest: &Cidr, interface: &str) -> Result<(), String> {
    run(ROUTE, &route_args("delete", dest, interface))
}

fn delete_route_any(dest: &Cidr) -> Result<(), String> {
    let family = if dest.is_ipv4() { "-inet" } else { "-inet6" };
    let kind = if dest.is_host() { "-host" } else { "-net" };
    run(ROUTE, &["-q".into(), "-n".into(), "delete".into(), family.into(), kind.into(), dest.to_string()])
}

/// Give the tunnel interface an IPv6 address so IPv6 routes can use it.
pub fn enable_ipv6(interface: &str) -> Result<(), String> {
    run(IFCONFIG, &[interface.into(), "inet6".into(), TUN_IPV6.into(), "prefixlen".into(), "128".into()])
}

pub fn interface_index(name: &str) -> Option<std::num::NonZeroU32> {
    let c = std::ffi::CString::new(name).ok()?;
    std::num::NonZeroU32::new(unsafe { libc::if_nametoindex(c.as_ptr()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_route_get_output() {
        let out = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.1.1\n  interface: en0\n      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>\n";
        assert_eq!(
            parse_route_get(out),
            DefaultRoute { interface: Some("en0".into()), gateway: Some("192.168.1.1".into()) }
        );
        assert_eq!(parse_route_get("route: writing to routing socket: not in table"), DefaultRoute::default());
    }

    #[test]
    fn builds_route_commands() {
        let host: Cidr = "188.114.96.5".parse().unwrap();
        assert_eq!(route_args("add", &host, "utun7").join(" "), "-q -n add -inet -host 188.114.96.5 -interface utun7");
        let net: Cidr = "2a06:98c1:3120::/48".parse().unwrap();
        assert_eq!(
            route_args("delete", &net, "utun7").join(" "),
            "-q -n delete -inet6 -net 2a06:98c1:3120::/48 -interface utun7"
        );
    }
}
