//! Drive the detour state machine against a fake platform.

use detour_core::plan::Probe;
use detour_core::protocol::Phase;
use detour_core::{Cidr, Config};
use detourd::engine::{Engine, Network, Platform, TunGuard, TunnelCtl};
use detourd::system::DefaultRoute;
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct World {
    routes: BTreeSet<Cidr>,
    deleted: Vec<Cidr>,
    tuns_created: usize,
    tun_alive: Option<Arc<AtomicBool>>,
    tunnel_up: bool,
    tunnel_restarts: usize,
    tunnel_stopped: usize,
    list: Result<String, String>,
    probes: HashMap<Cidr, Probe>,
    direct_network: bool,
    network: Network,
    fail_route: Option<Cidr>,
    fetched_via_socks: Vec<Option<u16>>,
    network_reads: usize,
}

#[derive(Clone)]
struct Fake(Arc<Mutex<World>>);

impl Fake {
    fn new() -> Self {
        let w = World {
            list: Ok("188.114.96.5\n188.114.97.5\n104.21.0.1\n93.184.216.34\n".into()),
            direct_network: true,
            network: Network {
                ipv4: DefaultRoute { interface: Some("en0".into()), gateway: Some("192.168.1.1".into()) },
                ipv6: DefaultRoute::default(),
            },
            routes: BTreeSet::new(),
            deleted: Vec::new(),
            tuns_created: 0,
            tun_alive: None,
            tunnel_up: false,
            tunnel_restarts: 0,
            tunnel_stopped: 0,
            probes: HashMap::new(),
            fail_route: None,
            fetched_via_socks: Vec::new(),
            network_reads: 0,
        };
        let fake = Fake(Arc::new(Mutex::new(w)));
        fake.set_probe("188.114.96.5", Probe::Blocked);
        fake.set_probe("188.114.97.5", Probe::Blocked);
        fake.set_probe("104.21.0.1", Probe::Reachable);
        fake.set_probe("93.184.216.34", Probe::Blocked);
        fake
    }

    fn w(&self) -> std::sync::MutexGuard<'_, World> {
        self.0.lock().unwrap()
    }

    fn set_probe(&self, ip: &str, probe: Probe) {
        self.w().probes.insert(ip.parse().unwrap(), probe);
    }

    fn routes(&self) -> Vec<String> {
        self.w().routes.iter().map(|c| c.to_string()).collect()
    }
}

struct FakeTun(Arc<AtomicBool>);

impl TunGuard for FakeTun {
    fn name(&self) -> &str {
        "utun9"
    }
    fn alive(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    fn index(&self) -> Option<u16> {
        Some(9)
    }
}

struct FakeTunnel(Fake);

impl TunnelCtl for FakeTunnel {
    fn poll(&mut self) -> impl Future<Output = bool> + Send {
        let up = self.0.w().tunnel_up;
        async move { up }
    }
    fn restart_now(&mut self) -> impl Future<Output = ()> + Send {
        self.0.w().tunnel_restarts += 1;
        async {}
    }
    fn stop(&mut self) -> impl Future<Output = ()> + Send {
        self.0.w().tunnel_stopped += 1;
        async {}
    }
    fn reconnects(&self) -> u32 {
        0
    }
    fn connected_since(&self) -> Option<u64> {
        self.0.w().tunnel_up.then_some(1)
    }
    fn last_error(&self) -> Option<String> {
        None
    }
}

impl Platform for Fake {
    type Tun = FakeTun;
    type Tunnel = FakeTunnel;

    fn start_tun(&self, _cfg: &Config) -> Result<FakeTun, String> {
        let alive = Arc::new(AtomicBool::new(true));
        let mut w = self.w();
        w.tuns_created += 1;
        w.tun_alive = Some(alive.clone());
        Ok(FakeTun(alive))
    }
    fn tunnel(&self, _cfg: &Config) -> FakeTunnel {
        FakeTunnel(self.clone())
    }
    fn add_route(&self, dest: &Cidr, interface: &str) -> Result<(), String> {
        assert_eq!(interface, "utun9");
        let mut w = self.w();
        if w.fail_route == Some(*dest) {
            return Err("boom".into());
        }
        w.routes.insert(*dest);
        Ok(())
    }
    fn delete_route(&self, dest: &Cidr, _interface: &str) -> Result<(), String> {
        let mut w = self.w();
        w.routes.remove(dest);
        w.deleted.push(*dest);
        Ok(())
    }
    fn network(&self) -> Network {
        let mut w = self.w();
        w.network_reads += 1;
        w.network.clone()
    }
    fn fetch_list(&self, _url: String, socks: Option<u16>) -> impl Future<Output = Result<String, String>> + Send {
        let mut w = self.w();
        w.fetched_via_socks.push(socks);
        let list = w.list.clone();
        async move { list }
    }
    fn probe(
        &self,
        entries: Vec<Cidr>,
        _network: Network,
        control: Option<SocketAddr>,
        _timeout: Duration,
    ) -> impl Future<Output = Option<Vec<(Cidr, Option<Probe>)>>> + Send {
        let w = self.w();
        assert_eq!(control, Some("93.184.216.34:22".parse().unwrap()), "control probe targets the tunnel server");
        let out = w
            .direct_network
            .then(|| entries.into_iter().map(|c| (c, Some(*w.probes.get(&c).unwrap_or(&Probe::Reachable)))).collect());
        async move { out }
    }
    fn now_unix(&self) -> u64 {
        1_760_112_000
    }
}

fn config() -> Config {
    Config { ssh_host: "93.184.216.34".into(), ssh_user: "tunnel".into(), ..Config::default() }
}

#[tokio::test(start_paused = true)]
async fn detours_only_blocked_destinations_and_never_the_server() {
    let fake = Fake::new();
    let (engine, handle, rx) = Engine::new(config(), fake.clone(), None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));

    let status = handle.set_enabled(true).await.unwrap();
    assert!(status.enabled);
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert_eq!(fake.routes(), vec!["188.114.96.5", "188.114.97.5"], "reachable and server addresses stay direct");
    let status = handle.status();
    assert_eq!(status.routes.interface.as_deref(), Some("utun9"));
    assert_eq!(status.routes.ipv4, 2);
    assert_eq!(status.list.listed, 4);
    assert_eq!(status.list.reachable, 1);
    assert_eq!(status.phase, Phase::Connecting, "tunnel not up yet");

    fake.w().tunnel_up = true;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(handle.status().phase, Phase::Active);

    handle.set_enabled(false).await.unwrap();
    assert!(fake.routes().is_empty(), "switching off removes every route");
    assert_eq!(fake.w().tunnel_stopped, 1);
    assert_eq!(handle.status().phase, Phase::Off);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn refreshes_add_new_blocks_without_removing_old_routes() {
    let fake = Fake::new();
    let mut cfg = config();
    cfg.refresh_secs = 60;
    let (engine, handle, rx) = Engine::new(cfg, fake.clone(), None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    handle.set_enabled(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(fake.routes(), vec!["188.114.96.5", "188.114.97.5"]);

    // The block moves: one address is unblocked and drops off the list, a new one appears.
    fake.w().list = Ok("188.114.97.5\n104.18.0.1\n".into());
    fake.set_probe("104.18.0.1", Probe::Blocked);
    fake.set_probe("188.114.96.5", Probe::Reachable);
    tokio::time::sleep(Duration::from_secs(65)).await;

    assert_eq!(
        fake.routes(),
        vec!["104.18.0.1", "188.114.96.5", "188.114.97.5"],
        "routes are sticky while on: removing one would cut live connections"
    );
    assert!(fake.w().deleted.is_empty());
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn no_direct_network_means_no_new_routes() {
    let fake = Fake::new();
    fake.w().direct_network = false;
    let (engine, handle, rx) = Engine::new(config(), fake.clone(), None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    handle.set_enabled(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(fake.routes().is_empty(), "an offline probe must not mark everything blocked");
    assert!(handle.status().last_error.unwrap().contains("no direct connection"));
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn failed_downloads_keep_existing_routes_and_retry() {
    let fake = Fake::new();
    let mut cfg = config();
    cfg.extra_routes = vec!["188.114.96.0/20".parse().unwrap()];
    let (engine, handle, rx) = Engine::new(cfg, fake.clone(), None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    fake.w().list = Err("HTTP 503".into());
    handle.set_enabled(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(fake.routes(), vec!["188.114.96.0/20"], "extra routes go in immediately");
    assert!(handle.status().last_error.unwrap().contains("HTTP 503"));

    fake.w().list = Ok("104.18.0.1\n".into());
    fake.set_probe("104.18.0.1", Probe::Blocked);
    tokio::time::sleep(Duration::from_secs(65)).await;
    assert_eq!(fake.routes(), vec!["104.18.0.1", "188.114.96.0/20"], "retried after a minute");
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn network_change_restarts_the_tunnel_and_reprobes() {
    let fake = Fake::new();
    let (engine, handle, rx) = Engine::new(config(), fake.clone(), None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    handle.set_enabled(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let fetches = fake.w().fetched_via_socks.len();

    fake.w().network.ipv4 = DefaultRoute { interface: Some("en1".into()), gateway: Some("192.168.1.1".into()) };
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(fake.w().tunnel_restarts, 1, "a new network gets a fresh ssh session at once");
    assert!(fake.w().fetched_via_socks.len() > fetches, "and a fresh probe");
    assert_eq!(handle.status().primary_interface.as_deref(), Some("en1"));
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_dead_interface_is_recreated_with_its_routes() {
    let fake = Fake::new();
    let (engine, handle, rx) = Engine::new(config(), fake.clone(), None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    handle.set_enabled(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let alive = fake.w().tun_alive.clone().unwrap();
    alive.store(false, Ordering::SeqCst);
    // Interface death takes its routes with it.
    fake.w().routes.clear();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(fake.w().tuns_created, 2);
    assert_eq!(fake.routes(), vec!["188.114.96.5", "188.114.97.5"]);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn persisted_state_restores_the_detour_and_shutdown_keeps_it() {
    let dir = std::env::temp_dir().join(format!("macdetour-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = dir.join("state.json");
    std::fs::write(&state, r#"{"enabled":true}"#).unwrap();

    let fake = Fake::new();
    let (engine, handle, rx) = Engine::new(config(), fake.clone(), Some(state.clone())).handle_pair();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(engine.run(rx, async move {
        let _ = stop_rx.await;
    }));
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(handle.status().enabled, "restored at startup");
    assert_eq!(fake.routes().len(), 2);

    stop_tx.send(()).unwrap();
    task.await.unwrap();
    assert!(fake.routes().is_empty(), "shutdown cleans up routes");
    assert_eq!(std::fs::read_to_string(&state).unwrap(), r#"{"enabled":true}"#, "but remembers it was on");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(start_paused = true)]
async fn refresh_requires_the_detour_to_be_on() {
    let fake = Fake::new();
    let (engine, handle, rx) = Engine::new(config(), fake, None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    assert!(handle.refresh().await.unwrap_err().contains("off"));
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_failed_route_is_reported_and_others_still_install() {
    let fake = Fake::new();
    fake.w().fail_route = Some("188.114.96.5".parse().unwrap());
    let (engine, handle, rx) = Engine::new(config(), fake.clone(), None).handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    handle.set_enabled(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(fake.routes(), vec!["188.114.97.5"]);
    assert!(handle.status().last_error.unwrap().contains("188.114.96.5"));
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn routing_events_drive_network_checks_instead_of_polling() {
    let fake = Fake::new();
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(8);
    let engine = Engine::new(config(), fake.clone(), None).with_network_events(events_rx);
    let (engine, handle, rx) = engine.handle_pair();
    let task = tokio::spawn(engine.run(rx, std::future::pending()));
    handle.set_enabled(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let reads = fake.w().network_reads;

    // Quiet network: no reads between fallback checks.
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(fake.w().network_reads, reads, "no polling without events");

    // Events about our own detour routes (utun index 9) are ignored.
    for _ in 0..50 {
        events_tx.send(9).await.unwrap();
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(fake.w().network_reads, reads, "own routes never trigger a check");

    // A burst of foreign events is debounced into one check that sees the change.
    fake.w().network.ipv4 = DefaultRoute { interface: Some("en1".into()), gateway: Some("10.0.0.1".into()) };
    for _ in 0..5 {
        events_tx.send(4).await.unwrap();
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(fake.w().network_reads, reads + 1, "one debounced read");
    assert_eq!(fake.w().tunnel_restarts, 1);
    assert_eq!(handle.status().primary_interface.as_deref(), Some("en1"));

    // A missed event is still caught by the fallback check.
    fake.w().network.ipv4 = DefaultRoute { interface: Some("en0".into()), gateway: Some("192.168.1.1".into()) };
    tokio::time::sleep(Duration::from_secs(62)).await;
    assert_eq!(handle.status().primary_interface.as_deref(), Some("en0"));
    drop(handle);
    task.await.unwrap();
}
