//! Keeps one `ssh -N -D` session to the tunnel server alive.
//!
//! The session is restarted with backoff when it dies, and immediately when
//! the network changes, so a Wi-Fi switch costs seconds instead of the
//! keep-alive timeout.

use detour_core::Config;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub const SSH: &str = "/usr/bin/ssh";
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A session that lasted this long resets the backoff.
const STABLE_AFTER: Duration = Duration::from_secs(60);

pub fn ssh_args(cfg: &Config) -> Vec<String> {
    let opt = |kv: &str| ["-o".to_string(), kv.to_string()];
    let mut args = vec![
        "-N".to_string(),
        // Ignore any ssh_config: the daemon must behave the same everywhere.
        "-F".into(),
        "/dev/null".into(),
        "-D".into(),
        format!("127.0.0.1:{}", cfg.socks_port),
        "-i".into(),
        cfg.ssh_key.clone(),
        "-p".into(),
        cfg.ssh_port.to_string(),
        "-l".into(),
        cfg.ssh_user.clone(),
    ];
    for kv in [
        "BatchMode=yes",
        "IdentitiesOnly=yes",
        "ExitOnForwardFailure=yes",
        "StrictHostKeyChecking=yes",
        // ssh splits this option on whitespace (it takes a list of files), and the
        // default path has a space in "Application Support": quote it.
        &format!("UserKnownHostsFile=\"{}\"", cfg.ssh_known_hosts),
        "GlobalKnownHostsFile=/dev/null",
        "ConnectTimeout=10",
        "ServerAliveInterval=10",
        "ServerAliveCountMax=3",
        "TCPKeepAlive=yes",
        "ControlMaster=no",
        "ControlPath=none",
        "LogLevel=ERROR",
    ] {
        args.extend(opt(kv));
    }
    args.push(cfg.ssh_host.clone());
    args
}

pub fn next_backoff(current: Duration) -> Duration {
    if current.is_zero() {
        Duration::from_secs(1)
    } else {
        (current * 2).min(MAX_BACKOFF)
    }
}

pub struct Tunnel {
    cfg: Config,
    child: Option<Child>,
    started: Option<Instant>,
    connected_since: Option<u64>,
    restart_at: Instant,
    backoff: Duration,
    pub reconnects: u32,
    last_stderr: Arc<Mutex<String>>,
    pub last_error: Option<String>,
}

impl Tunnel {
    pub fn new(cfg: Config) -> Self {
        Tunnel {
            cfg,
            child: None,
            started: None,
            connected_since: None,
            restart_at: Instant::now(),
            backoff: Duration::ZERO,
            reconnects: 0,
            last_stderr: Arc::new(Mutex::new(String::new())),
            last_error: None,
        }
    }

    pub fn server(&self) -> String {
        format!("{}@{}", self.cfg.ssh_user, self.cfg.ssh_host)
    }

    pub fn connected_since(&self) -> Option<u64> {
        self.connected_since
    }

    fn spawn(&mut self) {
        let mut cmd = Command::new(SSH);
        cmd.args(ssh_args(&self.cfg))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(stderr) = child.stderr.take() {
                    let sink = self.last_stderr.clone();
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(stderr).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let line = line.trim().to_string();
                            if !line.is_empty() {
                                log::warn!("ssh: {line}");
                                *sink.lock().unwrap_or_else(|e| e.into_inner()) = line;
                            }
                        }
                    });
                }
                log::info!("ssh session starting to {}", self.server());
                self.child = Some(child);
                self.started = Some(Instant::now());
            }
            Err(e) => {
                self.last_error = Some(format!("cannot start ssh: {e}"));
                self.schedule_restart();
            }
        }
    }

    fn schedule_restart(&mut self) {
        if self.started.is_some_and(|s| s.elapsed() >= STABLE_AFTER) {
            self.backoff = Duration::ZERO;
        }
        self.backoff = next_backoff(self.backoff);
        self.restart_at = Instant::now() + self.backoff;
        self.started = None;
    }

    /// Restart now (the network changed): skip the backoff.
    pub async fn restart_now(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            self.reconnects += 1;
        }
        self.connected_since = None;
        self.backoff = Duration::ZERO;
        self.restart_at = Instant::now();
    }

    /// Advance the supervisor. Returns whether the SOCKS proxy is usable.
    pub async fn poll(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    let detail = self.last_stderr.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let detail = if detail.is_empty() { format!("ssh exited with {status}") } else { detail };
                    log::warn!("ssh session ended: {detail}");
                    self.last_error = Some(detail);
                    self.child = None;
                    self.connected_since = None;
                    self.reconnects += 1;
                    self.schedule_restart();
                }
                Err(e) => {
                    self.last_error = Some(format!("ssh wait failed: {e}"));
                }
            }
        }
        if self.child.is_none() && Instant::now() >= self.restart_at {
            self.spawn();
        }
        if self.child.is_none() {
            return false;
        }
        // Once up, a live ssh process means a live proxy: ssh exits when the
        // session dies (ServerAlive), so there is no need to dial it every tick.
        if self.connected_since.is_some() {
            return true;
        }
        let up = socks_listening(self.cfg.socks_port).await;
        if up && self.connected_since.is_none() {
            log::info!("tunnel up via {}", self.server());
            self.connected_since = Some(unix_now());
            self.last_error = None;
        } else if !up && self.connected_since.is_some() {
            self.connected_since = None;
        }
        up
    }

    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        self.connected_since = None;
    }
}

pub async fn socks_listening(port: u16) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_millis(500), tokio::net::TcpStream::connect(("127.0.0.1", port))).await,
        Ok(Ok(_))
    )
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_locked_down_ssh_command() {
        let cfg = Config { ssh_host: "93.184.216.34".into(), ssh_user: "tunnel".into(), ..Config::default() };
        let args = ssh_args(&cfg);
        let joined = args.join(" ");
        assert!(joined.starts_with("-N -F /dev/null -D 127.0.0.1:41080 -i "));
        for needle in [
            "-l tunnel",
            "-p 22",
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "UserKnownHostsFile=\"/Library/Application Support/MacDetour/known_hosts\"",
            "ExitOnForwardFailure=yes",
            "ServerAliveInterval=10",
        ] {
            assert!(joined.contains(needle), "missing {needle}");
        }
        assert_eq!(args.last().unwrap(), "93.184.216.34", "host comes last, after every option");
    }

    #[test]
    fn backoff_doubles_up_to_a_cap() {
        let mut b = Duration::ZERO;
        let mut seen = vec![];
        for _ in 0..8 {
            b = next_backoff(b);
            seen.push(b.as_secs());
        }
        assert_eq!(seen, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }
}
