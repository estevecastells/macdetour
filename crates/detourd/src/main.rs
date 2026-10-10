//! detourd: runs as root under launchd.
//!
//!   detourd [--config PATH] [--socket PATH] [--state PATH]

use detour_core::protocol::DEFAULT_SOCKET;
use detour_core::Config;
use detourd::engine::Engine;
use detourd::platform::MacPlatform;
use std::path::PathBuf;

const DEFAULT_CONFIG: &str = "/Library/Application Support/MacDetour/config.toml";
const DEFAULT_STATE: &str = "/Library/Application Support/MacDetour/state.json";

fn arg(args: &[String], name: &str, default: &str) -> PathBuf {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| default.into())
}

#[tokio::main]
async fn main() {
    detourd::logger::init();
    let args: Vec<String> = std::env::args().collect();
    let config_path = arg(&args, "--config", DEFAULT_CONFIG);
    let socket_path = arg(&args, "--socket", DEFAULT_SOCKET);
    let state_path = arg(&args, "--state", DEFAULT_STATE);

    let cfg = match std::fs::read_to_string(&config_path).map_err(|e| e.to_string()).and_then(|t| Config::from_toml(&t))
    {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!("invalid config {}: {e}", config_path.display());
            std::process::exit(78); // EX_CONFIG: launchd will not hot-loop on a bad file
        }
    };
    let allowed = cfg.allowed_uids.clone();
    let (engine, handle, rx) = Engine::new(cfg, MacPlatform, Some(state_path)).handle_pair();
    if let Err(e) = detourd::server::spawn(handle, &socket_path, allowed) {
        log::error!("cannot listen on {}: {e}", socket_path.display());
        std::process::exit(1);
    }
    log::info!("detourd {} listening on {}", env!("CARGO_PKG_VERSION"), socket_path.display());

    let shutdown = async {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {},
            _ = tokio::signal::ctrl_c() => {},
        }
        log::info!("shutting down");
    };
    engine.run(rx, shutdown).await;
    let _ = std::fs::remove_file(&socket_path);
}
