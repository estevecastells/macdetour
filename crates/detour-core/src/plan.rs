//! Decide which destinations go through the tunnel.
//!
//! The plan is deliberately sticky: while the detour is on, a destination
//! that was detoured stays detoured, even if it drops off the list or starts
//! answering directly again. Removing a route would cut every connection that
//! is flowing through it, so routes are only ever added until the detour is
//! switched off. Destinations that answer directly are never detoured, so
//! connections that already work are never interrupted.

use crate::net::{is_never_detoured, Cidr};
use std::collections::BTreeSet;

/// What a direct connection attempt to a listed host found on this network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// A direct TCP connection succeeded: leave it alone.
    Reachable,
    /// The connection timed out, was refused or reset: treat it as blocked.
    Blocked,
    /// The address family is not usable here (no IPv6 route, for example).
    Unsupported,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanInput<'a> {
    /// Destinations currently routed through the tunnel.
    pub current: BTreeSet<Cidr>,
    /// Listed destinations, with the probe result when verification ran.
    pub candidates: Vec<(Cidr, Option<Probe>)>,
    /// Always detoured while on.
    pub extra: &'a [Cidr],
    /// Never detoured (includes the tunnel server itself).
    pub never: &'a [Cidr],
    pub max_routes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// New routes to install.
    pub add: Vec<Cidr>,
    /// Listed destinations left alone because they answer directly.
    pub reachable: usize,
    /// Listed destinations skipped because this network cannot use them anyway.
    pub unsupported: usize,
    /// Destinations dropped because `max_routes` was reached.
    pub over_limit: usize,
}

pub fn plan(input: &PlanInput<'_>) -> Plan {
    let mut out = Plan::default();
    let mut total = input.current.len();
    let mut seen: BTreeSet<Cidr> = BTreeSet::new();
    let excluded = |c: &Cidr| is_never_detoured(c.addr()) || input.never.iter().any(|n| n.overlaps(c));

    let wanted = input
        .extra
        .iter()
        .map(|c| (*c, None))
        .chain(input.candidates.iter().copied().filter_map(|(c, probe)| match probe {
            Some(Probe::Reachable) => {
                out.reachable += 1;
                None
            }
            Some(Probe::Unsupported) => {
                out.unsupported += 1;
                None
            }
            Some(Probe::Blocked) | None => Some((c, probe)),
        }))
        .collect::<Vec<_>>();

    for (cidr, _) in wanted {
        if input.current.contains(&cidr) || excluded(&cidr) || !seen.insert(cidr) {
            continue;
        }
        if total >= input.max_routes {
            out.over_limit += 1;
            continue;
        }
        out.add.push(cidr);
        total += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> Cidr {
        s.parse().unwrap()
    }

    #[test]
    fn detours_only_blocked_or_unverified_candidates() {
        let input = PlanInput {
            candidates: vec![
                (c("188.114.96.5"), Some(Probe::Blocked)),
                (c("188.114.97.5"), None),
                (c("104.21.0.1"), Some(Probe::Reachable)),
                (c("2a06:98c1:3120::5"), Some(Probe::Unsupported)),
            ],
            max_routes: 100,
            ..Default::default()
        };
        let p = plan(&input);
        assert_eq!(p.add, vec![c("188.114.96.5"), c("188.114.97.5")]);
        assert_eq!((p.reachable, p.unsupported, p.over_limit), (1, 1, 0));
    }

    #[test]
    fn routes_are_sticky_and_never_duplicated() {
        let input = PlanInput {
            current: [c("188.114.96.5")].into(),
            candidates: vec![(c("188.114.96.5"), Some(Probe::Blocked)), (c("188.114.97.5"), Some(Probe::Blocked))],
            max_routes: 100,
            ..Default::default()
        };
        assert_eq!(plan(&input).add, vec![c("188.114.97.5")]);
    }

    #[test]
    fn never_detours_the_tunnel_server_or_excluded_ranges() {
        let never = [c("93.184.216.34"), c("104.16.0.0/13")];
        let input = PlanInput {
            candidates: vec![
                (c("93.184.216.34"), Some(Probe::Blocked)),
                (c("104.21.5.5"), Some(Probe::Blocked)),
                (c("192.168.1.1"), Some(Probe::Blocked)),
                (c("188.114.96.5"), Some(Probe::Blocked)),
            ],
            never: &never,
            max_routes: 100,
            ..Default::default()
        };
        assert_eq!(plan(&input).add, vec![c("188.114.96.5")]);
    }

    #[test]
    fn extra_routes_come_first_and_the_limit_holds() {
        let extra = [c("188.114.96.0/20")];
        let input = PlanInput {
            current: [c("1.2.3.4")].into(),
            candidates: vec![(c("104.21.0.1"), Some(Probe::Blocked)), (c("104.18.0.1"), Some(Probe::Blocked))],
            extra: &extra,
            max_routes: 3,
            ..Default::default()
        };
        let p = plan(&input);
        assert_eq!(p.add, vec![c("188.114.96.0/20"), c("104.21.0.1")]);
        assert_eq!(p.over_limit, 1);
    }

    #[test]
    fn empty_input_is_a_no_op() {
        assert_eq!(plan(&PlanInput { max_routes: 10, ..Default::default() }), Plan::default());
    }
}
