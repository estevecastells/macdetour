//! Wire protocol: one JSON object per line over `/var/run/macdetour.sock`.
//! Samples live in `/fixtures` and are checked from both Rust and Swift.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_SOCKET: &str = "/var/run/macdetour.sock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    SetEnabled {
        enabled: bool,
    },
    /// Fetch the list and probe again now instead of waiting for the timer.
    Refresh,
}

impl Request {
    pub fn is_mutation(&self) -> bool {
        matches!(self, Request::SetEnabled { .. } | Request::Refresh)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong { version: String, protocol: u32 },
    Status { status: Status },
    Error { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Switched off: no tunnel, no routes.
    Off,
    /// Switched on, waiting for the tunnel to come up.
    Connecting,
    /// Tunnel up and routes installed.
    Active,
    /// Switched on but the tunnel is down; detoured destinations wait for it.
    Reconnecting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub enabled: bool,
    pub phase: Phase,
    pub tunnel: TunnelStatus,
    pub routes: RouteStatus,
    pub list: ListStatus,
    /// Network interface used for direct traffic (e.g. `en0`).
    pub primary_interface: Option<String>,
    /// Most recent problem worth showing, if any.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TunnelStatus {
    pub connected: bool,
    /// Unix seconds when the current SSH session came up.
    pub connected_since: Option<u64>,
    pub reconnects: u32,
    pub server: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteStatus {
    pub interface: Option<String>,
    pub ipv4: usize,
    pub ipv6: usize,
    /// A few detoured destinations, for display.
    pub sample: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListStatus {
    /// Unix seconds of the last successful download.
    pub fetched_at: Option<u64>,
    pub listed: usize,
    /// Listed destinations that answer directly on this network.
    pub reachable: usize,
    /// Listed destinations this network cannot use (e.g. IPv6 without IPv6).
    pub unsupported: usize,
    pub refreshing: bool,
}

impl Status {
    pub fn off(server: &str) -> Self {
        Status {
            enabled: false,
            phase: Phase::Off,
            tunnel: TunnelStatus { server: server.into(), ..Default::default() },
            routes: RouteStatus::default(),
            list: ListStatus::default(),
            primary_interface: None,
            last_error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap()
    }

    #[test]
    fn requests_match_fixtures() {
        let reqs: Vec<Request> = serde_json::from_str(&fixture("requests.json")).unwrap();
        assert_eq!(reqs, vec![Request::Ping, Request::Status, Request::SetEnabled { enabled: true }, Request::Refresh]);
        assert!(!Request::Status.is_mutation());
        assert!(Request::SetEnabled { enabled: false }.is_mutation());
        assert!(Request::Refresh.is_mutation());
    }

    #[test]
    fn status_fixture_round_trips() {
        let resp: Response = serde_json::from_str(&fixture("status.json")).unwrap();
        let Response::Status { status } = &resp else { panic!("expected status") };
        assert_eq!(status.phase, Phase::Active);
        assert_eq!(status.routes.ipv4, 2);
        let again: Response = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert_eq!(again, resp);
    }

    #[test]
    fn unknown_request_types_are_rejected() {
        assert!(serde_json::from_str::<Request>(r#"{"type":"shutdown"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"type":"set_enabled"}"#).is_err());
    }
}
