//! Unix-socket server: newline-delimited JSON requests and responses.
//!
//! The socket is world-connectable so the menu bar app can read the status,
//! but switching the detour on or off is limited to root and `allowed_uids`.

use crate::engine::Handle;
use detour_core::protocol::{Request, Response, PROTOCOL_VERSION};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

const MAX_LINE: u64 = 64 * 1024;

pub async fn handle(handle: &Handle, req: Request, peer_uid: u32, allowed_uids: &[u32]) -> Response {
    if req.is_mutation() && peer_uid != 0 && !allowed_uids.contains(&peer_uid) {
        return Response::Error { message: format!("permission denied for uid {peer_uid}") };
    }
    let result = match req {
        Request::Ping => {
            return Response::Pong { version: env!("CARGO_PKG_VERSION").into(), protocol: PROTOCOL_VERSION }
        }
        Request::Status => Ok(handle.status()),
        Request::SetEnabled { enabled } => handle.set_enabled(enabled).await,
        Request::Refresh => handle.refresh().await,
    };
    match result {
        Ok(status) => Response::Status { status },
        Err(message) => Response::Error { message },
    }
}

async fn serve_connection(handle: Handle, stream: UnixStream, allowed_uids: Vec<u32>) {
    let Ok(cred) = stream.peer_cred() else { return };
    let uid = cred.uid();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    loop {
        line.clear();
        let mut limited = (&mut reader).take(MAX_LINE);
        match tokio::time::timeout(Duration::from_secs(60), limited.read_line(&mut line)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return,
            Ok(Ok(_)) => {}
        }
        if !line.ends_with('\n') && line.len() as u64 >= MAX_LINE {
            let _ = write.write_all(encode(&Response::Error { message: "request too large".into() }).as_bytes()).await;
            return;
        }
        let resp = match serde_json::from_str::<Request>(line.trim()) {
            Ok(req) => self::handle(&handle, req, uid, &allowed_uids).await,
            Err(e) => Response::Error { message: format!("bad request: {e}") },
        };
        if write.write_all(encode(&resp).as_bytes()).await.is_err() {
            return;
        }
    }
}

fn encode(r: &Response) -> String {
    let mut s = serde_json::to_string(r).unwrap_or_else(|e| format!(r#"{{"type":"error","message":"{e}"}}"#));
    s.push('\n');
    s
}

/// Bind the socket (replacing a stale one) and serve until the runtime stops.
pub fn spawn(handle: Handle, path: &Path, allowed_uids: Vec<u32>) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            tokio::spawn(serve_connection(handle.clone(), stream, allowed_uids.clone()));
        }
    });
    Ok(())
}
