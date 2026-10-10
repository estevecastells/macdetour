//! Parse the public list of addresses blocked in Spain (one per line).

use crate::net::{is_never_detoured, Cidr};
use std::collections::BTreeSet;

/// Refuse lists larger than this many bytes; the real list is a few kilobytes.
pub const MAX_LIST_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Blocklist {
    pub entries: BTreeSet<Cidr>,
    /// Lines that were not addresses (kept as a count, for diagnostics).
    pub skipped: usize,
}

/// Parse `text`, accepting single addresses or CIDR networks. Comments (`#`),
/// blank lines, unparseable lines and local or reserved ranges are skipped.
/// Networks broader than /16 (IPv4) or /32 (IPv6) are skipped too, so a bad
/// list can never pull large parts of the internet into the tunnel.
pub fn parse(text: &str) -> Result<Blocklist, String> {
    if text.len() > MAX_LIST_BYTES {
        return Err(format!("list is larger than {MAX_LIST_BYTES} bytes"));
    }
    let mut list = Blocklist::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Ok(cidr) = line.parse::<Cidr>() else {
            list.skipped += 1;
            continue;
        };
        let too_broad = cidr.prefix() < if cidr.is_ipv4() { 16 } else { 32 };
        if too_broad || is_never_detoured(cidr.addr()) {
            list.skipped += 1;
            continue;
        }
        list.entries.insert(cidr);
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_published_format() {
        let text = "2a06:98c1:3120::\n2a06:98c1:3120::5\n188.114.96.5\n188.114.97.5\n188.114.96.5\n";
        let list = parse(text).unwrap();
        assert_eq!(list.entries.len(), 4, "duplicates collapse");
        assert_eq!(list.skipped, 0);
        assert!(list.entries.contains(&"188.114.97.5".parse().unwrap()));
    }

    #[test]
    fn skips_noise_local_ranges_and_broad_networks() {
        let text = "# comment\n\n  104.21.0.1  # trailing\n<html>\n10.0.0.1\n127.0.0.1\n1.0.0.0/8\n::/0\n2a06::/16\n188.114.96.0/20\n";
        let list = parse(text).unwrap();
        let got: Vec<String> = list.entries.iter().map(|c| c.to_string()).collect();
        assert_eq!(got, vec!["104.21.0.1", "188.114.96.0/20"]);
        assert_eq!(list.skipped, 6);
    }

    #[test]
    fn an_html_error_page_yields_nothing() {
        let list = parse("<!doctype html><html><body>Not found</body></html>").unwrap();
        assert!(list.entries.is_empty());
    }

    #[test]
    fn rejects_oversized_input() {
        let big = "1.1.1.1\n".repeat(MAX_LIST_BYTES / 8 + 1);
        assert!(parse(&big).is_err());
    }
}
