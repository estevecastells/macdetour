//! Direct reachability probes that bypass the detour.
//!
//! Each probe socket is bound to the physical interface (`IP_BOUND_IF`), so it
//! leaves through the normal network even when a detour route for the same
//! address already exists.

use detour_core::plan::Probe;
use detour_core::Cidr;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const PARALLEL_PROBES: usize = 48;

/// Interfaces available for direct traffic, per address family.
#[derive(Debug, Clone, Copy, Default)]
pub struct Egress {
    pub ipv4: Option<NonZeroU32>,
    pub ipv6: Option<NonZeroU32>,
}

pub fn probe_one(addr: SocketAddr, egress: Egress, timeout: Duration) -> Probe {
    let (domain, index) = match addr.ip() {
        IpAddr::V4(_) => (Domain::IPV4, egress.ipv4),
        IpAddr::V6(_) => (Domain::IPV6, egress.ipv6),
    };
    let Some(index) = index else { return Probe::Unsupported };
    let Ok(socket) = Socket::new(domain, Type::STREAM, Some(Protocol::TCP)) else { return Probe::Blocked };
    let bound = match addr.ip() {
        IpAddr::V4(_) => socket.bind_device_by_index_v4(Some(index)),
        IpAddr::V6(_) => socket.bind_device_by_index_v6(Some(index)),
    };
    if bound.is_err() {
        return Probe::Unsupported;
    }
    match socket.connect_timeout(&addr.into(), timeout) {
        Ok(()) => Probe::Reachable,
        Err(e) if e.raw_os_error() == Some(libc::ENETUNREACH) || e.raw_os_error() == Some(libc::EHOSTUNREACH) => {
            // No route on this network for the family: not a block.
            if addr.is_ipv6() {
                Probe::Unsupported
            } else {
                Probe::Blocked
            }
        }
        Err(_) => Probe::Blocked,
    }
}

/// Probe every host entry on TCP 443. Networks (non-host entries) cannot be
/// probed and come back as `None` (detour without verification).
pub async fn probe_all(entries: Vec<Cidr>, egress: Egress, timeout: Duration) -> Vec<(Cidr, Option<Probe>)> {
    let limit = Arc::new(Semaphore::new(PARALLEL_PROBES));
    let mut tasks = Vec::with_capacity(entries.len());
    for cidr in entries {
        let limit = limit.clone();
        tasks.push(tokio::spawn(async move {
            if !cidr.is_host() {
                return (cidr, None);
            }
            let Ok(_permit) = limit.acquire_owned().await else { return (cidr, None) };
            let addr = SocketAddr::new(cidr.addr(), 443);
            let result =
                tokio::task::spawn_blocking(move || probe_one(addr, egress, timeout)).await.unwrap_or(Probe::Blocked);
            (cidr, Some(result))
        }));
    }
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(result) = task.await {
            out.push(result);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn loopback() -> Egress {
        let lo = crate::system::interface_index("lo0");
        Egress { ipv4: lo, ipv6: lo }
    }

    #[test]
    fn reachable_when_something_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(probe_one(addr, loopback(), Duration::from_secs(2)), Probe::Reachable);
    }

    #[test]
    fn blocked_when_nothing_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        assert_eq!(probe_one(addr, loopback(), Duration::from_secs(2)), Probe::Blocked);
    }

    #[test]
    fn unsupported_without_an_interface_for_the_family() {
        let egress = Egress { ipv4: loopback().ipv4, ipv6: None };
        let addr: SocketAddr = "[2a06:98c1:3120::5]:443".parse().unwrap();
        assert_eq!(probe_one(addr, egress, Duration::from_secs(1)), Probe::Unsupported);
    }

    #[tokio::test]
    async fn networks_are_not_probed() {
        let out = probe_all(vec!["188.114.96.0/20".parse().unwrap()], Egress::default(), Duration::from_secs(1)).await;
        assert_eq!(out, vec![("188.114.96.0/20".parse().unwrap(), None)]);
    }
}
