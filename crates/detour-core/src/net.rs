//! A minimal CIDR type. Host routes are the common case (`/32`, `/128`).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Build a network, masking off host bits so equal networks compare equal.
    pub fn new(addr: IpAddr, prefix: u8) -> Result<Self, String> {
        let max = Self::max_prefix(&addr);
        if prefix > max {
            return Err(format!("prefix /{prefix} is too long for {addr}"));
        }
        let addr = match addr {
            IpAddr::V4(a) => {
                let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
                IpAddr::V4(Ipv4Addr::from(u32::from(a) & mask))
            }
            IpAddr::V6(a) => {
                let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
                IpAddr::V6(Ipv6Addr::from(u128::from(a) & mask))
            }
        };
        Ok(Cidr { addr, prefix })
    }

    pub fn host(addr: IpAddr) -> Self {
        Cidr { addr, prefix: Self::max_prefix(&addr) }
    }

    fn max_prefix(addr: &IpAddr) -> u8 {
        if addr.is_ipv4() {
            32
        } else {
            128
        }
    }

    pub fn addr(&self) -> IpAddr {
        self.addr
    }

    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    pub fn is_host(&self) -> bool {
        self.prefix == Self::max_prefix(&self.addr)
    }

    pub fn is_ipv4(&self) -> bool {
        self.addr.is_ipv4()
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - self.prefix) };
                u32::from(ip) & mask == u32::from(net)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - self.prefix) };
                u128::from(ip) & mask == u128::from(net)
            }
            _ => false,
        }
    }

    /// True when the two networks share at least one address.
    pub fn overlaps(&self, other: &Cidr) -> bool {
        if self.addr.is_ipv4() != other.addr.is_ipv4() {
            return false;
        }
        if self.prefix <= other.prefix {
            self.contains(other.addr)
        } else {
            other.contains(self.addr)
        }
    }
}

impl FromStr for Cidr {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        match s.split_once('/') {
            Some((addr, prefix)) => {
                let addr: IpAddr = addr.parse().map_err(|_| format!("invalid address in {s:?}"))?;
                let prefix: u8 = prefix.parse().map_err(|_| format!("invalid prefix in {s:?}"))?;
                Cidr::new(addr, prefix)
            }
            None => s.parse::<IpAddr>().map(Cidr::host).map_err(|_| format!("invalid address {s:?}")),
        }
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_host() {
            write!(f, "{}", self.addr)
        } else {
            write!(f, "{}/{}", self.addr, self.prefix)
        }
    }
}

impl Serialize for Cidr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Cidr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Address classes that must never be sent through the tunnel: local,
/// private, link-local, multicast and documentation ranges. Routing any of
/// them would break the LAN or the tunnel itself.
pub fn is_never_detoured(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            a.is_unspecified()
                || a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_multicast()
                || a.is_broadcast()
                || a.is_documentation()
                || a.octets()[0] == 0
                // Carrier-grade NAT (100.64.0.0/10), used by hotspots and Tailscale.
                || (a.octets()[0] == 100 && (a.octets()[1] & 0xC0) == 64)
                // Benchmarking (198.18.0.0/15) and reserved (240.0.0.0/4).
                || (a.octets()[0] == 198 && (a.octets()[1] & 0xFE) == 18)
                || a.octets()[0] >= 240
        }
        IpAddr::V6(a) => {
            let seg0 = a.segments()[0];
            a.is_unspecified()
                || a.is_loopback()
                || a.is_multicast()
                || (seg0 & 0xFE00) == 0xFC00 // unique local
                || (seg0 & 0xFFC0) == 0xFE80 // link local
                || (seg0 == 0x2001 && a.segments()[1] == 0x0DB8) // documentation
                || a.to_ipv4_mapped().is_some()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_and_networks() {
        let host: Cidr = "188.114.96.5".parse().unwrap();
        assert!(host.is_host());
        assert_eq!(host.to_string(), "188.114.96.5");

        let net: Cidr = "188.114.96.7/20".parse().unwrap();
        assert_eq!(net.to_string(), "188.114.96.0/20");
        assert!(net.contains("188.114.111.255".parse().unwrap()));
        assert!(!net.contains("188.114.112.0".parse().unwrap()));

        let v6: Cidr = "2a06:98c1:3120::5".parse().unwrap();
        assert_eq!(v6.prefix(), 128);
        let v6net: Cidr = "2a06:98c1:3120::/44".parse().unwrap();
        assert!(v6net.contains("2a06:98c1:3121::5".parse().unwrap()));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "1.2.3", "1.2.3.4/33", "::1/129", "a.b.c.d", "1.2.3.4/x", "1.2.3.4/-1"] {
            assert!(bad.parse::<Cidr>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn overlap_is_symmetric_and_family_aware() {
        let a: Cidr = "10.0.0.0/8".parse().unwrap();
        let b: Cidr = "10.1.2.3".parse().unwrap();
        let c: Cidr = "11.0.0.0/8".parse().unwrap();
        let v6: Cidr = "::/0".parse().unwrap();
        assert!(a.overlaps(&b) && b.overlaps(&a));
        assert!(!a.overlaps(&c));
        assert!(!a.overlaps(&v6));
    }

    #[test]
    fn never_detours_local_and_special_ranges() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "192.168.1.1",
            "169.254.1.1",
            "100.100.100.100",
            "224.0.0.1",
            "255.255.255.255",
            "198.18.0.1",
            "0.1.2.3",
            "::1",
            "fe80::1",
            "fd7a:115c:a1e0::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:1.2.3.4",
        ] {
            assert!(is_never_detoured(ip.parse().unwrap()), "{ip} must stay local");
        }
        for ip in ["188.114.96.5", "104.21.0.1", "2a06:98c1:3120::5", "2606:4700::1"] {
            assert!(!is_never_detoured(ip.parse().unwrap()), "{ip} is public");
        }
    }

    #[test]
    fn serde_round_trip() {
        let c: Cidr = "104.16.0.0/13".parse().unwrap();
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"104.16.0.0/13\"");
        assert_eq!(serde_json::from_str::<Cidr>(&json).unwrap(), c);
    }
}
