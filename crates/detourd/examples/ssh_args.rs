//! Print the exact ssh command the daemon would run, for manual checks:
//!   cargo run --example ssh_args -- <host> <key> <known_hosts> [port]
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let cfg = detour_core::Config {
        ssh_host: a[0].clone(),
        ssh_key: a[1].clone(),
        ssh_known_hosts: a[2].clone(),
        socks_port: a.get(3).and_then(|p| p.parse().ok()).unwrap_or(41080),
        ..Default::default()
    };
    for arg in detourd::tunnel::ssh_args(&cfg) {
        println!("{arg}");
    }
}
