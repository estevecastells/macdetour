//! The control socket: permissions, requests and malformed input.

use detour_core::protocol::{Request, Response, Status};
use detourd::engine::{Command, Handle};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

fn socket_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("macdetour-{name}-{}.sock", std::process::id()))
}

/// A fake engine that flips `enabled` and answers with the status.
fn fake_engine() -> Handle {
    let status = Arc::new(Mutex::new(Status::off("tunnel@example")));
    let (tx, mut rx) = mpsc::channel::<Command>(4);
    let cell = status.clone();
    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                Command::SetEnabled { enabled, reply } => {
                    cell.lock().unwrap().enabled = enabled;
                    let _ = reply.send(Ok(cell.lock().unwrap().clone()));
                }
                Command::Refresh { reply } => {
                    let _ = reply.send(Err("the detour is off".into()));
                }
            }
        }
    });
    Handle::from_parts(tx, status)
}

fn ask(path: &PathBuf, line: &str) -> Response {
    let mut stream = UnixStream::connect(path).unwrap();
    // The server may answer and hang up before an oversized request is fully
    // written, so a failed write is fine as long as the reply arrives.
    let _ = stream.write_all(line.as_bytes()).and_then(|_| stream.write_all(b"\n"));
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap();
    serde_json::from_str(&reply).unwrap()
}

fn req(r: &Request) -> String {
    serde_json::to_string(r).unwrap()
}

fn my_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[tokio::test(flavor = "multi_thread")]
async fn anyone_can_read_status_but_only_allowed_users_switch() {
    let path = socket_path("perm");
    detourd::server::spawn(fake_engine(), &path, vec![]).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o666);

    let p = path.clone();
    let (pong, status, denied) = tokio::task::spawn_blocking(move || {
        (
            ask(&p, &req(&Request::Ping)),
            ask(&p, &req(&Request::Status)),
            ask(&p, &req(&Request::SetEnabled { enabled: true })),
        )
    })
    .await
    .unwrap();
    assert!(matches!(pong, Response::Pong { protocol: 1, .. }));
    assert!(matches!(status, Response::Status { status } if !status.enabled));
    if my_uid() != 0 {
        assert!(matches!(denied, Response::Error { message } if message.contains("permission denied")));
    }
    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "multi_thread")]
async fn allowed_users_switch_and_see_engine_errors() {
    let path = socket_path("allowed");
    detourd::server::spawn(fake_engine(), &path, vec![my_uid()]).unwrap();
    let p = path.clone();
    let (on, refresh) = tokio::task::spawn_blocking(move || {
        (ask(&p, &req(&Request::SetEnabled { enabled: true })), ask(&p, &req(&Request::Refresh)))
    })
    .await
    .unwrap();
    assert!(matches!(on, Response::Status { status } if status.enabled));
    assert!(matches!(refresh, Response::Error { message } if message.contains("off")));
    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_and_oversized_requests_are_rejected() {
    let path = socket_path("bad");
    detourd::server::spawn(fake_engine(), &path, vec![]).unwrap();
    let p = path.clone();
    let (garbage, unknown, huge) = tokio::task::spawn_blocking(move || {
        (ask(&p, "not json"), ask(&p, r#"{"type":"self_destruct"}"#), ask(&p, &"x".repeat(70 * 1024)))
    })
    .await
    .unwrap();
    assert!(matches!(garbage, Response::Error { message } if message.starts_with("bad request")));
    assert!(matches!(unknown, Response::Error { .. }));
    assert!(matches!(huge, Response::Error { message } if message.contains("too large")));
    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_socket_file_is_replaced() {
    let path = socket_path("stale");
    std::fs::write(&path, b"stale").unwrap();
    detourd::server::spawn(fake_engine(), &path, vec![]).unwrap();
    let p = path.clone();
    let pong = tokio::task::spawn_blocking(move || ask(&p, &req(&Request::Ping))).await.unwrap();
    assert!(matches!(pong, Response::Pong { .. }));
    let _ = std::fs::remove_file(path);
}
