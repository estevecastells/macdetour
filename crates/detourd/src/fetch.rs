//! Download the blocklist with the system curl: through the tunnel first (the
//! list's own host can be blocked), then directly.

use std::time::Duration;
use tokio::process::Command;

pub const CURL: &str = "/usr/bin/curl";

pub fn curl_args(url: &str, socks_port: Option<u16>) -> Vec<String> {
    let mut args: Vec<String> = [
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--max-redirs",
        "3",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-time",
        "20",
        "--max-filesize",
        "1048576",
        "--user-agent",
        "MacDetour/0.1 (+personal use)",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(port) = socks_port {
        args.push("--socks5-hostname".into());
        args.push(format!("127.0.0.1:{port}"));
    }
    args.push("--".into());
    args.push(url.into());
    args
}

async fn curl(url: &str, socks_port: Option<u16>) -> Result<String, String> {
    let fut = Command::new(CURL).args(curl_args(url, socks_port)).kill_on_drop(true).output();
    let out = tokio::time::timeout(Duration::from_secs(30), fut)
        .await
        .map_err(|_| "list download timed out".to_string())?
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    String::from_utf8(out.stdout).map_err(|_| "list is not UTF-8".to_string())
}

pub async fn fetch_list(url: &str, socks_port: Option<u16>) -> Result<String, String> {
    if let Some(port) = socks_port {
        match curl(url, Some(port)).await {
            Ok(text) => return Ok(text),
            Err(e) => log::warn!("list download through the tunnel failed ({e}); trying directly"),
        }
    }
    curl(url, None).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn curl_is_https_only_and_bounded() {
        let args = super::curl_args("https://example.com/list.txt", Some(41080)).join(" ");
        assert!(args.contains("--proto =https --proto-redir =https"));
        assert!(args.contains("--max-filesize 1048576"));
        assert!(args.contains("--socks5-hostname 127.0.0.1:41080"));
        assert!(args.ends_with("-- https://example.com/list.txt"));
        assert!(!super::curl_args("https://example.com/", None).join(" ").contains("socks"));
    }
}
