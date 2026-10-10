//! The real macOS [`Platform`]: utun + tun2proxy, `route`, direct probes and curl.

use crate::engine::{Candidates, Network, Platform, TunGuard, TunnelCtl};
use crate::probe::{self, Egress};
use crate::system;
use crate::tunnel::{unix_now, Tunnel};
use detour_core::plan::Probe;
use detour_core::{Cidr, Config};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tun2proxy::{Args, CancellationToken, ProxyParameters};

const TUN_MTU: u16 = 1500;
/// Point-to-point addresses for the utun interface. Nothing is routed to them;
/// they only bring the interface up.
const TUN_ADDR: (u8, u8, u8, u8) = (10, 233, 233, 1);
const TUN_PEER: (u8, u8, u8, u8) = (10, 233, 233, 2);
const TUN_MASK: (u8, u8, u8, u8) = (255, 255, 255, 252);
const MAX_SESSIONS: usize = 4096;

#[derive(Clone, Default)]
pub struct MacPlatform;

pub struct MacTun {
    name: String,
    index: Option<u16>,
    token: CancellationToken,
    alive: Arc<AtomicBool>,
}

impl TunGuard for MacTun {
    fn name(&self) -> &str {
        &self.name
    }

    fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn index(&self) -> Option<u16> {
        self.index
    }
}

impl Drop for MacTun {
    fn drop(&mut self) {
        // Stopping the stack drops the device, which destroys the interface.
        self.token.cancel();
    }
}

impl TunnelCtl for Tunnel {
    fn poll(&mut self) -> impl Future<Output = bool> + Send {
        Tunnel::poll(self)
    }

    fn restart_now(&mut self) -> impl Future<Output = ()> + Send {
        Tunnel::restart_now(self)
    }

    fn stop(&mut self) -> impl Future<Output = ()> + Send {
        Tunnel::stop(self)
    }

    fn reconnects(&self) -> u32 {
        self.reconnects
    }

    fn connected_since(&self) -> Option<u64> {
        Tunnel::connected_since(self)
    }

    fn last_error(&self) -> Option<String> {
        self.last_error.clone()
    }
}

impl Platform for MacPlatform {
    type Tun = MacTun;
    type Tunnel = Tunnel;

    fn start_tun(&self, cfg: &Config) -> Result<MacTun, String> {
        let mut tun_config = tun::Configuration::default();
        tun_config.address(TUN_ADDR).netmask(TUN_MASK).destination(TUN_PEER).mtu(TUN_MTU).up();
        // utun always prefixes packets with a 4-byte address-family header;
        // `true` tells the tun crate to strip it on read and add it on write.
        tun_config.platform_config(|c| {
            c.packet_information(true);
        });
        let device = tun::create_as_async(&tun_config).map_err(|e| e.to_string())?;
        let name = tun::AbstractDevice::tun_name(&*device).map_err(|e| e.to_string())?;
        if let Err(e) = system::enable_ipv6(&name) {
            log::warn!("IPv6 detours unavailable on {name}: {e}");
        }

        let mut args = Args::default();
        let proxy = ProxyParameters::try_from(format!("socks5://127.0.0.1:{}", cfg.socks_port).as_str())
            .map_err(|e| e.to_string())?;
        args.proxy(proxy);
        args.max_sessions = MAX_SESSIONS;

        let token = CancellationToken::new();
        let alive = Arc::new(AtomicBool::new(true));
        let (task_token, task_alive, task_name) = (token.clone(), alive.clone(), name.clone());
        tokio::spawn(async move {
            match tun2proxy::run(device, TUN_MTU, args, task_token).await {
                Ok(sessions) => log::info!("{task_name} stopped ({sessions} open sessions)"),
                Err(e) => log::error!("{task_name} stopped: {e}"),
            }
            task_alive.store(false, Ordering::SeqCst);
        });
        let index = system::interface_index(&name).and_then(|i| u16::try_from(i.get()).ok());
        Ok(MacTun { name, index, token, alive })
    }

    fn tunnel(&self, cfg: &Config) -> Tunnel {
        Tunnel::new(cfg.clone())
    }

    fn add_route(&self, dest: &Cidr, interface: &str) -> Result<(), String> {
        system::add_route(dest, interface)
    }

    fn delete_route(&self, dest: &Cidr, interface: &str) -> Result<(), String> {
        system::delete_route(dest, interface)
    }

    fn network(&self) -> Network {
        Network { ipv4: system::default_route(false), ipv6: system::default_route(true) }
    }

    async fn fetch_list(&self, url: String, socks_port: Option<u16>) -> Result<String, String> {
        crate::fetch::fetch_list(&url, socks_port).await
    }

    async fn probe(
        &self,
        entries: Vec<Cidr>,
        network: Network,
        control: Option<SocketAddr>,
        timeout: Duration,
    ) -> Option<Candidates> {
        let index = |route: &system::DefaultRoute| route.interface.as_deref().and_then(system::interface_index);
        let egress = Egress { ipv4: index(&network.ipv4), ipv6: index(&network.ipv6) };
        egress.ipv4.or(egress.ipv6)?;
        // A probe that cannot even reach the tunnel server means there is no
        // working direct network; its results would mark everything blocked.
        if let Some(control) = control {
            if probe::probe_one(control, egress, timeout).await != Probe::Reachable {
                return None;
            }
        }
        Some(probe::probe_all(entries, egress, timeout).await)
    }

    fn now_unix(&self) -> u64 {
        unix_now()
    }
}
