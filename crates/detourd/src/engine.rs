//! The detour state machine.
//!
//! Switching on creates the utun interface, starts the SSH tunnel and then
//! adds routes only for listed destinations that do not answer directly.
//! While on, routes are only ever added (see `detour_core::plan`), so a
//! refresh never interrupts a connection. Switching off removes every route
//! and tears the interface and tunnel down.
//!
//! Everything that touches the OS goes through [`Platform`], so the state
//! machine is tested with a fake.

use crate::system::DefaultRoute;
use detour_core::blocklist;
use detour_core::plan::{plan, PlanInput, Probe};
use detour_core::protocol::{ListStatus, Phase, RouteStatus, Status, TunnelStatus};
use detour_core::{Cidr, Config};
use std::collections::BTreeSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

pub const TICK: Duration = Duration::from_secs(2);
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);
/// Wait for routing events to settle before reading the default route again.
const NETWORK_DEBOUNCE: Duration = Duration::from_millis(500);
/// Re-read the default route at least this often, in case an event was missed.
const NETWORK_FALLBACK: Duration = Duration::from_secs(60);
const SAMPLE_SIZE: usize = 6;

/// Listed destinations with their direct-probe result (`None` when unverified).
pub type Candidates = Vec<(Cidr, Option<Probe>)>;

/// A running utun interface plus its userspace TCP stack. Dropping it
/// destroys the interface, which also removes every route through it.
pub trait TunGuard: Send {
    fn name(&self) -> &str;
    fn alive(&self) -> bool;
    /// Interface index, used to ignore routing events about our own routes.
    fn index(&self) -> Option<u16> {
        None
    }
}

pub trait TunnelCtl: Send {
    /// Advance the supervisor; true when the SOCKS proxy is usable.
    fn poll(&mut self) -> impl Future<Output = bool> + Send;
    fn restart_now(&mut self) -> impl Future<Output = ()> + Send;
    fn stop(&mut self) -> impl Future<Output = ()> + Send;
    fn reconnects(&self) -> u32;
    fn connected_since(&self) -> Option<u64>;
    fn last_error(&self) -> Option<String>;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Network {
    pub ipv4: DefaultRoute,
    pub ipv6: DefaultRoute,
}

pub trait Platform: Clone + Send + Sync + 'static {
    type Tun: TunGuard;
    type Tunnel: TunnelCtl;

    fn start_tun(&self, cfg: &Config) -> Result<Self::Tun, String>;
    fn tunnel(&self, cfg: &Config) -> Self::Tunnel;
    fn add_route(&self, dest: &Cidr, interface: &str) -> Result<(), String>;
    fn delete_route(&self, dest: &Cidr, interface: &str) -> Result<(), String>;
    fn network(&self) -> Network;
    fn fetch_list(&self, url: String, socks_port: Option<u16>) -> impl Future<Output = Result<String, String>> + Send;
    /// Probe host entries directly; `None` when there is no direct network at all.
    fn probe(
        &self,
        entries: Vec<Cidr>,
        network: Network,
        control: Option<std::net::SocketAddr>,
        timeout: Duration,
    ) -> impl Future<Output = Option<Candidates>> + Send;
    fn now_unix(&self) -> u64;
}

pub enum Command {
    SetEnabled { enabled: bool, reply: oneshot::Sender<Result<Status, String>> },
    Refresh { reply: oneshot::Sender<Result<Status, String>> },
}

#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Command>,
    status: Arc<Mutex<Status>>,
}

impl Handle {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<Status, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::SetEnabled { enabled, reply })
            .await
            .map_err(|_| "daemon is shutting down".to_string())?;
        rx.await.map_err(|_| "daemon is shutting down".to_string())?
    }

    pub async fn refresh(&self) -> Result<Status, String> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(Command::Refresh { reply }).await.map_err(|_| "daemon is shutting down".to_string())?;
        rx.await.map_err(|_| "daemon is shutting down".to_string())?
    }

    /// A handle backed by a caller-owned channel, for socket tests.
    pub fn from_parts(tx: mpsc::Sender<Command>, status: Arc<Mutex<Status>>) -> Self {
        Handle { tx, status }
    }
}

struct RefreshResult {
    candidates: Result<(Candidates, usize), String>,
}

struct Active<P: Platform> {
    tun: P::Tun,
    tunnel: P::Tunnel,
    tunnel_up: bool,
    routes: BTreeSet<Cidr>,
    next_refresh: Instant,
    refreshing: bool,
    list: ListStatus,
}

pub struct Engine<P: Platform> {
    cfg: Config,
    platform: P,
    status: Arc<Mutex<Status>>,
    state_path: Option<PathBuf>,
    active: Option<Active<P>>,
    network: Network,
    last_error: Option<String>,
    refresh_tx: mpsc::Sender<RefreshResult>,
    refresh_rx: mpsc::Receiver<RefreshResult>,
    /// Kernel routing events (interface index). Without them, every tick polls.
    net_events: Option<mpsc::Receiver<u16>>,
    net_check_due: Option<Instant>,
    last_net_check: Option<Instant>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct PersistedState {
    enabled: bool,
}

impl<P: Platform> Engine<P> {
    pub fn new(cfg: Config, platform: P, state_path: Option<PathBuf>) -> Self {
        let status = Arc::new(Mutex::new(Status::off(&format!("{}@{}", cfg.ssh_user, cfg.ssh_host))));
        let (refresh_tx, refresh_rx) = mpsc::channel(4);
        Engine {
            cfg,
            platform,
            status,
            state_path,
            active: None,
            network: Network::default(),
            last_error: None,
            refresh_tx,
            refresh_rx,
            net_events: None,
            net_check_due: None,
            last_net_check: None,
        }
    }

    /// Read the default route only after routing events instead of on every tick.
    pub fn with_network_events(mut self, events: mpsc::Receiver<u16>) -> Self {
        self.net_events = Some(events);
        self
    }

    fn note_network_event(&mut self, index: u16) {
        // Our own detour routes generate an event each; they never move the default route.
        if self.active.as_ref().and_then(|a| a.tun.index()) == Some(index) {
            return;
        }
        self.net_check_due.get_or_insert_with(|| Instant::now() + NETWORK_DEBOUNCE);
    }

    fn network_check_due(&self) -> bool {
        if self.net_events.is_none() {
            return true;
        }
        let now = Instant::now();
        self.net_check_due.is_some_and(|due| now >= due)
            || self.last_net_check.is_none_or(|last| now >= last + NETWORK_FALLBACK)
    }

    async fn next_network_event(events: &mut Option<mpsc::Receiver<u16>>) -> Option<u16> {
        match events.as_mut() {
            Some(rx) => rx.recv().await,
            None => std::future::pending().await,
        }
    }

    pub fn status_cell(&self) -> Arc<Mutex<Status>> {
        self.status.clone()
    }

    pub fn is_enabled(&self) -> bool {
        self.active.is_some()
    }

    pub fn routes(&self) -> Vec<Cidr> {
        self.active.as_ref().map(|a| a.routes.iter().copied().collect()).unwrap_or_default()
    }

    /// Whether the last saved state asked for the detour to be on.
    pub fn persisted_enabled(&self) -> bool {
        let Some(path) = &self.state_path else { return false };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<PersistedState>(&s).ok())
            .is_some_and(|s| s.enabled)
    }

    fn persist(&self, enabled: bool) {
        let Some(path) = &self.state_path else { return };
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string(&PersistedState { enabled }).unwrap_or_default();
        if let Err(e) = std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, path)) {
            log::warn!("could not save state to {}: {e}", path.display());
        }
    }

    pub async fn enable(&mut self) -> Result<(), String> {
        if self.active.is_some() {
            return Ok(());
        }
        let tun = self.platform.start_tun(&self.cfg).map_err(|e| {
            let msg = format!("cannot create the tunnel interface: {e}");
            self.last_error = Some(msg.clone());
            msg
        })?;
        log::info!("detour on via {}", tun.name());
        let mut active = Active {
            tunnel: self.platform.tunnel(&self.cfg),
            tun,
            tunnel_up: false,
            routes: BTreeSet::new(),
            next_refresh: Instant::now(),
            refreshing: false,
            list: ListStatus::default(),
        };
        self.network = self.platform.network();
        self.last_error = None;
        let extra = self.cfg.extra_routes.clone();
        self.install(&mut active, &extra);
        self.active = Some(active);
        self.persist(true);
        self.publish();
        Ok(())
    }

    pub async fn disable(&mut self) {
        if let Some(mut active) = self.active.take() {
            for dest in &active.routes {
                if let Err(e) = self.platform.delete_route(dest, active.tun.name()) {
                    log::warn!("could not remove route to {dest}: {e}");
                }
            }
            active.tunnel.stop().await;
            log::info!("detour off; removed {} routes", active.routes.len());
            drop(active);
        }
        self.persist(false);
        self.publish();
    }

    /// Add routes for `dests` that are not routed yet.
    fn install(&mut self, active: &mut Active<P>, dests: &[Cidr]) {
        let mut added = 0;
        for dest in dests {
            if active.routes.contains(dest) {
                continue;
            }
            match self.platform.add_route(dest, active.tun.name()) {
                Ok(()) => {
                    active.routes.insert(*dest);
                    added += 1;
                }
                Err(e) => {
                    log::warn!("could not route {dest}: {e}");
                    self.last_error = Some(format!("could not route {dest}: {e}"));
                }
            }
        }
        if added > 0 {
            log::info!("detouring {added} more destinations ({} in total)", active.routes.len());
        }
    }

    fn start_refresh(&mut self) {
        let Some(active) = self.active.as_mut() else { return };
        if active.refreshing {
            return;
        }
        active.refreshing = true;
        let platform = self.platform.clone();
        let cfg = self.cfg.clone();
        let network = self.network.clone();
        let socks = active.tunnel_up.then_some(cfg.socks_port);
        let tx = self.refresh_tx.clone();
        tokio::spawn(async move {
            let candidates = async {
                let text = platform.fetch_list(cfg.list_url.clone(), socks).await?;
                let list = blocklist::parse(&text)?;
                let listed = list.entries.len();
                let entries: Vec<Cidr> = list.entries.into_iter().collect();
                if !cfg.verify {
                    return Ok((entries.into_iter().map(|c| (c, None)).collect(), listed));
                }
                let control = cfg.ssh_host_ip().map(|ip| std::net::SocketAddr::new(ip, cfg.ssh_port));
                let timeout = Duration::from_secs(cfg.probe_timeout_secs);
                match platform.probe(entries, network, control, timeout).await {
                    Some(results) => Ok((results, listed)),
                    None => Err("no direct connection to verify against".to_string()),
                }
            }
            .await;
            let _ = tx.send(RefreshResult { candidates }).await;
        });
        self.publish();
    }

    fn apply_refresh(&mut self, result: RefreshResult) {
        let Some(mut active) = self.active.take() else { return };
        active.refreshing = false;
        match result.candidates {
            Ok((candidates, listed)) => {
                let mut never = self.cfg.never_routes.clone();
                if let Some(ip) = self.cfg.ssh_host_ip() {
                    never.push(Cidr::host(ip));
                }
                let p = plan(&PlanInput {
                    current: active.routes.clone(),
                    candidates,
                    extra: &self.cfg.extra_routes,
                    never: &never,
                    max_routes: self.cfg.max_routes,
                });
                active.list = ListStatus {
                    fetched_at: Some(self.platform.now_unix()),
                    listed,
                    reachable: p.reachable,
                    unsupported: p.unsupported,
                    refreshing: false,
                };
                if p.over_limit > 0 {
                    log::warn!("{} destinations skipped: max_routes reached", p.over_limit);
                }
                self.install(&mut active, &p.add);
                active.next_refresh = Instant::now() + Duration::from_secs(self.cfg.refresh_secs);
            }
            Err(e) => {
                log::warn!("refresh failed: {e}");
                self.last_error = Some(format!("list refresh failed: {e}"));
                active.next_refresh = Instant::now() + RETRY_AFTER_FAILURE;
            }
        }
        self.active = Some(active);
        self.publish();
    }

    pub async fn tick(&mut self) {
        let mut changed = false;
        if self.network_check_due() {
            self.net_check_due = None;
            self.last_net_check = Some(Instant::now());
            let network = self.platform.network();
            changed = network != self.network;
            if changed {
                self.note_network_change(network);
            }
        }
        self.tick_active(changed).await;
    }

    fn note_network_change(&mut self, network: Network) {
        log::info!(
            "network changed: {:?} -> {:?}",
            self.network.ipv4.interface.as_deref(),
            network.ipv4.interface.as_deref()
        );
        self.network = network;
    }

    async fn tick_active(&mut self, changed: bool) {
        let tun_dead = self.active.as_ref().is_some_and(|a| !a.tun.alive());
        if tun_dead {
            log::warn!("tunnel interface stopped; recreating it");
            let routes = self.routes();
            if let Some(mut old) = self.active.take() {
                old.tunnel.stop().await;
            }
            if self.enable().await.is_ok() {
                if let Some(mut active) = self.active.take() {
                    self.install(&mut active, &routes);
                    self.active = Some(active);
                }
            }
        }
        let Some(active) = self.active.as_mut() else { return };
        if changed {
            active.tunnel.restart_now().await;
            active.next_refresh = Instant::now();
        }
        active.tunnel_up = active.tunnel.poll().await;
        if Instant::now() >= active.next_refresh {
            self.start_refresh();
        }
        self.publish();
    }

    fn publish(&self) {
        let status = match &self.active {
            None => Status {
                last_error: self.last_error.clone(),
                ..Status::off(&format!("{}@{}", self.cfg.ssh_user, self.cfg.ssh_host))
            },
            Some(a) => {
                let phase = match (a.tunnel_up, a.tunnel.connected_since().is_some() || a.tunnel.reconnects() > 0) {
                    (true, _) => Phase::Active,
                    (false, true) => Phase::Reconnecting,
                    (false, false) => Phase::Connecting,
                };
                let ipv4 = a.routes.iter().filter(|c| c.is_ipv4()).count();
                Status {
                    enabled: true,
                    phase,
                    tunnel: TunnelStatus {
                        connected: a.tunnel_up,
                        connected_since: a.tunnel.connected_since(),
                        reconnects: a.tunnel.reconnects(),
                        server: format!("{}@{}", self.cfg.ssh_user, self.cfg.ssh_host),
                    },
                    routes: RouteStatus {
                        interface: Some(a.tun.name().to_string()),
                        ipv4,
                        ipv6: a.routes.len() - ipv4,
                        sample: a.routes.iter().take(SAMPLE_SIZE).map(|c| c.to_string()).collect(),
                    },
                    list: ListStatus { refreshing: a.refreshing, ..a.list.clone() },
                    primary_interface: self.network.ipv4.interface.clone(),
                    last_error: if a.tunnel_up {
                        self.last_error.clone()
                    } else {
                        a.tunnel.last_error().or_else(|| self.last_error.clone())
                    },
                }
            }
        };
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
    }

    async fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::SetEnabled { enabled, reply } => {
                let result = if enabled {
                    self.enable().await.map(|_| {
                        self.start_refresh();
                    })
                } else {
                    self.disable().await;
                    Ok(())
                };
                let _ = reply.send(result.map(|_| self.current()));
            }
            Command::Refresh { reply } => {
                if self.active.is_none() {
                    let _ = reply.send(Err("the detour is off".into()));
                    return;
                }
                self.start_refresh();
                let _ = reply.send(Ok(self.current()));
            }
        }
    }

    fn current(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Run until every handle is dropped or `shutdown` resolves, then switch off
    /// (without persisting, so the detour comes back after a restart).
    pub async fn run(mut self, mut rx: mpsc::Receiver<Command>, shutdown: impl Future<Output = ()>) {
        if self.persisted_enabled() {
            match self.enable().await {
                Ok(()) => self.start_refresh(),
                Err(e) => log::warn!("could not restore the detour: {e}"),
            }
        }
        self.publish();
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    Some(cmd) => self.handle(cmd).await,
                    None => break,
                },
                Some(result) = self.refresh_rx.recv() => self.apply_refresh(result),
                Some(index) = Self::next_network_event(&mut self.net_events) => self.note_network_event(index),
                _ = ticker.tick() => self.tick().await,
                _ = &mut shutdown => break,
            }
        }
        let was_enabled = self.is_enabled();
        self.disable().await;
        if was_enabled {
            self.persist(true);
        }
    }

    /// Split into the engine and a handle for the socket server.
    pub fn handle_pair(self) -> (Self, Handle, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel(16);
        let handle = Handle { tx, status: self.status.clone() };
        (self, handle, rx)
    }
}
