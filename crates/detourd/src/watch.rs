//! Network change notifications from the kernel routing socket (`PF_ROUTE`).
//!
//! Instead of asking `route` for the default route every few seconds, the
//! daemon sleeps until the kernel reports a route, address or interface
//! change. Each event carries the interface index it concerns, so the engine
//! can ignore the flood of events caused by its own detour routes.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

/// Interface index a routing message refers to, if it is a kind of change
/// that can move the default route: routes added, deleted or changed,
/// addresses added or removed, and interfaces going up or down.
pub fn relevant_index(msg: &[u8]) -> Option<u16> {
    if msg.len() < 4 || msg[2] != libc::RTM_VERSION as u8 {
        return None;
    }
    let kind = i32::from(msg[3]);
    match kind {
        libc::RTM_ADD | libc::RTM_DELETE | libc::RTM_CHANGE => {
            if msg.len() < std::mem::size_of::<libc::rt_msghdr>() {
                return None;
            }
            // SAFETY: length checked above; read_unaligned copes with any alignment.
            let hdr = unsafe { std::ptr::read_unaligned(msg.as_ptr().cast::<libc::rt_msghdr>()) };
            Some(hdr.rtm_index)
        }
        libc::RTM_NEWADDR | libc::RTM_DELADDR | libc::RTM_IFINFO => {
            if msg.len() < std::mem::size_of::<libc::ifa_msghdr>() {
                return None;
            }
            // SAFETY: as above. if_msghdr and ifa_msghdr share this prefix layout.
            let hdr = unsafe { std::ptr::read_unaligned(msg.as_ptr().cast::<libc::ifa_msghdr>()) };
            Some(hdr.ifam_index)
        }
        _ => None,
    }
}

/// Open the routing socket and forward the interface index of every relevant
/// message. Returns `None` when the socket cannot be opened (the engine then
/// falls back to polling).
pub fn spawn() -> Option<mpsc::Receiver<u16>> {
    let fd = unsafe { libc::socket(libc::PF_ROUTE, libc::SOCK_RAW, libc::AF_UNSPEC) };
    if fd < 0 {
        log::warn!("routing socket unavailable ({}); polling for network changes", std::io::Error::last_os_error());
        return None;
    }
    // SAFETY: fd is a freshly created socket we own.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    unsafe {
        let flags = libc::fcntl(owned.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(owned.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let async_fd = AsyncFd::new(owned).ok()?;
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            let Ok(mut guard) = async_fd.readable().await else { return };
            loop {
                let n = unsafe { libc::read(async_fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                        guard.clear_ready();
                        break;
                    }
                    return;
                }
                if let Some(index) = relevant_index(&buf[..n as usize]) {
                    // A full queue already guarantees a pending check: drop extras.
                    if tx.try_send(index).is_err() && tx.is_closed() {
                        return;
                    }
                }
            }
        }
    });
    Some(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt_message(kind: i32, index: u16) -> Vec<u8> {
        let mut hdr: libc::rt_msghdr = unsafe { std::mem::zeroed() };
        hdr.rtm_msglen = std::mem::size_of::<libc::rt_msghdr>() as u16;
        hdr.rtm_version = libc::RTM_VERSION as u8;
        hdr.rtm_type = kind as u8;
        hdr.rtm_index = index;
        let bytes = unsafe {
            std::slice::from_raw_parts((&hdr as *const libc::rt_msghdr).cast::<u8>(), std::mem::size_of_val(&hdr))
        };
        bytes.to_vec()
    }

    fn ifa_message(kind: i32, index: u16) -> Vec<u8> {
        let mut hdr: libc::ifa_msghdr = unsafe { std::mem::zeroed() };
        hdr.ifam_msglen = std::mem::size_of::<libc::ifa_msghdr>() as u16;
        hdr.ifam_version = libc::RTM_VERSION as u8;
        hdr.ifam_type = kind as u8;
        hdr.ifam_index = index;
        let bytes = unsafe {
            std::slice::from_raw_parts((&hdr as *const libc::ifa_msghdr).cast::<u8>(), std::mem::size_of_val(&hdr))
        };
        bytes.to_vec()
    }

    #[test]
    fn reports_route_and_interface_changes_with_their_index() {
        assert_eq!(relevant_index(&rt_message(libc::RTM_ADD, 7)), Some(7));
        assert_eq!(relevant_index(&rt_message(libc::RTM_DELETE, 4)), Some(4));
        assert_eq!(relevant_index(&rt_message(libc::RTM_CHANGE, 4)), Some(4));
        assert_eq!(relevant_index(&ifa_message(libc::RTM_NEWADDR, 11)), Some(11));
        assert_eq!(relevant_index(&ifa_message(libc::RTM_IFINFO, 12)), Some(12));
    }

    #[test]
    fn ignores_lookups_truncated_and_foreign_messages() {
        assert_eq!(relevant_index(&rt_message(libc::RTM_GET, 7)), None);
        assert_eq!(relevant_index(&rt_message(libc::RTM_MISS, 7)), None);
        assert_eq!(relevant_index(&rt_message(libc::RTM_ADD, 7)[..10]), None);
        assert_eq!(relevant_index(&[]), None);
        let mut wrong_version = rt_message(libc::RTM_ADD, 7);
        wrong_version[2] = 99;
        assert_eq!(relevant_index(&wrong_version), None);
    }

    #[tokio::test]
    async fn the_routing_socket_opens_without_root() {
        // Reading routing messages needs no privileges; only writing does.
        assert!(spawn().is_some());
    }
}
