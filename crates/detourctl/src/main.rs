//! detourctl: talk to the MacDetour daemon.
//!
//!   detourctl status [--json]
//!   detourctl on | off | refresh

use detour_core::protocol::{Phase, Request, Response, Status, DEFAULT_SOCKET};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

fn send(req: &Request) -> Result<Response, String> {
    let path = std::env::var("MACDETOUR_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.into());
    let mut stream = UnixStream::connect(&path).map_err(|e| format!("cannot reach the daemon at {path}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let mut line = serde_json::to_string(req).map_err(|e| e.to_string())?;
    line.push('\n');
    stream.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).map_err(|e| e.to_string())?;
    serde_json::from_str(&reply).map_err(|e| format!("unexpected reply: {e}"))
}

fn describe(s: &Status) -> String {
    let phase = match s.phase {
        Phase::Off => "off",
        Phase::Connecting => "connecting",
        Phase::Active => "active",
        Phase::Reconnecting => "reconnecting",
    };
    let mut out = format!("detour:     {phase}\n");
    out += &format!("tunnel:     {} ({})\n", if s.tunnel.connected { "up" } else { "down" }, s.tunnel.server);
    if s.enabled {
        out += &format!(
            "routes:     {} IPv4, {} IPv6 via {}\n",
            s.routes.ipv4,
            s.routes.ipv6,
            s.routes.interface.as_deref().unwrap_or("?")
        );
        if !s.routes.sample.is_empty() {
            out += &format!("            {}\n", s.routes.sample.join(", "));
        }
        out += &format!(
            "list:       {} listed, {} answer directly, {} not usable here{}\n",
            s.list.listed,
            s.list.reachable,
            s.list.unsupported,
            if s.list.refreshing { " (refreshing)" } else { "" }
        );
        out += &format!("network:    {}\n", s.primary_interface.as_deref().unwrap_or("none"));
    }
    if let Some(e) = &s.last_error {
        out += &format!("last error: {e}\n");
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let req = match args.first().map(String::as_str) {
        None | Some("status") | Some("--json") => Request::Status,
        Some("on") => Request::SetEnabled { enabled: true },
        Some("off") => Request::SetEnabled { enabled: false },
        Some("refresh") => Request::Refresh,
        Some("ping") => Request::Ping,
        Some(other) => {
            eprintln!("unknown command {other:?}\nusage: detourctl [status [--json] | on | off | refresh | ping]");
            return ExitCode::from(2);
        }
    };
    match send(&req) {
        Ok(Response::Status { status }) if json => {
            println!("{}", serde_json::to_string_pretty(&status).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Ok(Response::Status { status }) => {
            print!("{}", describe(&status));
            ExitCode::SUCCESS
        }
        Ok(Response::Pong { version, protocol }) => {
            println!("detourd {version} (protocol {protocol})");
            ExitCode::SUCCESS
        }
        Ok(Response::Error { message }) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
