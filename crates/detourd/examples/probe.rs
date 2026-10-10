//! Probe the live blocklist from this network without changing anything:
//!   cargo run --release --example probe
use detour_core::blocklist;
use detour_core::plan::Probe;
use detourd::probe::{probe_all, Egress};
use detourd::system;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let text = detourd::fetch::fetch_list(detour_core::config::DEFAULT_LIST_URL, None).await.expect("download");
    let list = blocklist::parse(&text).expect("parse");
    let v4 = system::default_route(false);
    let v6 = system::default_route(true);
    let index = |r: &system::DefaultRoute| r.interface.as_deref().and_then(system::interface_index);
    let egress = Egress { ipv4: index(&v4), ipv6: index(&v6) };
    println!(
        "listed {} (skipped {}), egress v4={:?} v6={:?}",
        list.entries.len(),
        list.skipped,
        v4.interface,
        v6.interface
    );
    let started = Instant::now();
    let results = probe_all(list.entries.into_iter().collect(), egress, Duration::from_secs(4)).await;
    let count = |p: Probe| results.iter().filter(|(_, r)| *r == Some(p)).count();
    println!(
        "blocked {} reachable {} unsupported {} in {:.1}s",
        count(Probe::Blocked),
        count(Probe::Reachable),
        count(Probe::Unsupported),
        started.elapsed().as_secs_f32()
    );
    for (c, r) in &results {
        if ["188.114.96.5", "188.114.97.5", "104.16.93.114"].contains(&c.to_string().as_str()) {
            println!("  {c}: {r:?}");
        }
    }
}
