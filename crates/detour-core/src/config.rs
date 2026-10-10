//! Daemon configuration (`/Library/Application Support/MacDetour/config.toml`).

use crate::net::Cidr;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

pub const DEFAULT_LIST_URL: &str = "https://hayahora.futbol/estado/blocked-any.txt";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// SSH server that carries the tunnel. Use an IP address: the daemon runs
    /// as root and does not read your personal `~/.ssh/config`.
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    /// Private key used only for this tunnel (restricted to port forwarding on the server).
    pub ssh_key: String,
    /// known_hosts file pinning the server key.
    pub ssh_known_hosts: String,
    /// Loopback port of the SOCKS proxy that `ssh -D` opens.
    pub socks_port: u16,
    /// Plain-text list of blocked addresses, one per line.
    pub list_url: String,
    /// Seconds between list refreshes while the detour is on.
    pub refresh_secs: u64,
    /// Detour a listed address only after a direct connection to it fails on
    /// this network. Working connections are never touched.
    pub verify: bool,
    /// Seconds a direct probe may take before the address counts as blocked.
    pub probe_timeout_secs: u64,
    /// Destinations that always go through the tunnel while it is on.
    pub extra_routes: Vec<Cidr>,
    /// Destinations that never go through the tunnel.
    pub never_routes: Vec<Cidr>,
    /// Upper bound on detoured destinations, as a guard against a bad list.
    pub max_routes: usize,
    /// Non-root users allowed to switch the detour on and off.
    pub allowed_uids: Vec<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_key: "/Library/Application Support/MacDetour/id_ed25519".into(),
            ssh_known_hosts: "/Library/Application Support/MacDetour/known_hosts".into(),
            socks_port: 41080,
            list_url: DEFAULT_LIST_URL.into(),
            refresh_secs: 300,
            verify: true,
            probe_timeout_secs: 4,
            extra_routes: Vec::new(),
            never_routes: Vec::new(),
            max_routes: 4096,
            allowed_uids: Vec::new(),
        }
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.ssh_host.trim().is_empty() {
            return Err("ssh_host is required".into());
        }
        if self.ssh_host.starts_with('-') || self.ssh_host.contains(char::is_whitespace) {
            return Err("ssh_host is not a valid host".into());
        }
        if self.ssh_user.is_empty()
            || self.ssh_user.starts_with('-')
            || !self.ssh_user.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            return Err("ssh_user is not a valid user name".into());
        }
        for (name, path) in [("ssh_key", &self.ssh_key), ("ssh_known_hosts", &self.ssh_known_hosts)] {
            // ssh_known_hosts is passed quoted inside an `-o` option.
            if !path.starts_with('/') || path.contains(['"', '\n', '\r']) {
                return Err(format!("{name} must be an absolute path without quotes"));
            }
        }
        if self.ssh_port == 0 {
            return Err("ssh_port must be non-zero".into());
        }
        if self.socks_port < 1024 {
            return Err("socks_port must be 1024 or higher".into());
        }
        if !self.list_url.starts_with("https://") {
            return Err("list_url must use https".into());
        }
        if !(60..=86_400).contains(&self.refresh_secs) {
            return Err("refresh_secs must be between 60 and 86400".into());
        }
        if !(1..=30).contains(&self.probe_timeout_secs) {
            return Err("probe_timeout_secs must be between 1 and 30".into());
        }
        if self.max_routes == 0 || self.max_routes > 65_536 {
            return Err("max_routes must be between 1 and 65536".into());
        }
        for route in &self.extra_routes {
            if route.prefix() < if route.is_ipv4() { 8 } else { 16 } {
                return Err(format!("extra route {route} is too broad"));
            }
            if crate::net::is_never_detoured(route.addr()) {
                return Err(format!("extra route {route} is a local or reserved range"));
            }
        }
        Ok(())
    }

    /// The tunnel server's own address, when `ssh_host` is a literal IP.
    /// It must never be detoured, or the tunnel would route into itself.
    pub fn ssh_host_ip(&self) -> Option<IpAddr> {
        self.ssh_host.trim_start_matches('[').trim_end_matches(']').parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> Config {
        Config { ssh_host: "93.184.216.34".into(), ssh_user: "tunnel".into(), ..Config::default() }
    }

    #[test]
    fn defaults_need_a_host_and_user() {
        assert!(Config::default().validate().is_err());
        assert!(valid().validate().is_ok());
    }

    #[test]
    fn toml_round_trip() {
        let mut cfg = valid();
        cfg.extra_routes = vec!["188.114.96.0/20".parse().unwrap()];
        cfg.allowed_uids = vec![501];
        let parsed = Config::from_toml(&cfg.to_toml()).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn partial_file_takes_defaults() {
        let cfg =
            Config::from_toml("ssh_host = \"93.184.216.34\"\nssh_user = \"tunnel\"\nallowed_uids = [501]\n").unwrap();
        assert_eq!(cfg.socks_port, 41080);
        assert!(cfg.verify);
        assert_eq!(cfg.allowed_uids, vec![501]);
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(Config::from_toml("ssh_host = \"h\"\nssh_user = \"u\"\nsurprise = 1\n").is_err());
        type Mutation = Box<dyn Fn(&mut Config)>;
        let cases: Vec<Mutation> = vec![
            Box::new(|c| c.ssh_host = "-oProxyCommand=x".into()),
            Box::new(|c| c.ssh_host = "a b".into()),
            Box::new(|c| c.ssh_user = "-l".into()),
            Box::new(|c| c.ssh_user = "us er".into()),
            Box::new(|c| c.ssh_port = 0),
            Box::new(|c| c.ssh_known_hosts = "/tmp/a\"b".into()),
            Box::new(|c| c.ssh_key = "relative/key".into()),
            Box::new(|c| c.socks_port = 80),
            Box::new(|c| c.list_url = "http://example.com/list.txt".into()),
            Box::new(|c| c.refresh_secs = 10),
            Box::new(|c| c.probe_timeout_secs = 0),
            Box::new(|c| c.max_routes = 0),
            Box::new(|c| c.extra_routes = vec!["0.0.0.0/0".parse().unwrap()]),
            Box::new(|c| c.extra_routes = vec!["192.168.0.0/16".parse().unwrap()]),
        ];
        for (i, mutate) in cases.iter().enumerate() {
            let mut cfg = valid();
            mutate(&mut cfg);
            assert!(cfg.validate().is_err(), "case {i} should be rejected");
        }
    }

    #[test]
    fn ssh_host_ip_is_only_for_literals() {
        assert_eq!(valid().ssh_host_ip(), Some("93.184.216.34".parse().unwrap()));
        let named = Config { ssh_host: "example.com".into(), ..Config::default() };
        assert_eq!(named.ssh_host_ip(), None);
    }
}
