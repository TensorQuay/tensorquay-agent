//! The clients of a desktop's display, told strictly (ADR-0039 section 10): a process is one when a socket of its own is
//! connected to the X server's socket, never because of `DISPLAY` in its environment. `/proc/net/unix` names the
//! server's listening socket by its path but not the connections it accepted, and no Unix socket's peer is there; so
//! the server is the process holding that listener, its other sockets are the connections it accepted, and the kernel's
//! socket diagnostics (`NETLINK_SOCK_DIAG`, `UNIX_DIAG_PEER`) give each one's peer, which is the client's end. Whatever
//! can't be read answers no clients, so the reaper takes everything as it did before.
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use rustix::net::{AddressFamily, RecvFlags, SendFlags, SocketType, netlink, recv, send, socket};

/// `SOCK_DIAG_BY_FAMILY`, the request for a family's sockets.
const SOCK_DIAG_BY_FAMILY: u16 = 20;
/// `NLM_F_REQUEST | NLM_F_DUMP`.
const DUMP: u16 = 0x0301;
/// `NLMSG_ERROR` and `NLMSG_DONE`.
const ERROR: u16 = 2;
const DONE: u16 = 3;
/// `UDIAG_SHOW_PEER`, and its answer's attribute `UNIX_DIAG_PEER`.
const SHOW_PEER: u32 = 0x4;
const PEER: u16 = 2;
/// `AF_UNIX`, as the request's family byte.
const UNIX: u8 = 1;
/// A netlink message's header, and a `unix_diag_msg` before its attributes.
const HEADER: usize = 16;
const MESSAGE: usize = 16;
/// `__SO_ACCEPTCON` in `/proc/net/unix`'s flags: a listening socket.
const ACCEPTING: u32 = 0x0001_0000;
/// The most answers read from one dump: a sandbox has far fewer sockets.
const MAX_READS: usize = 256;

/// The inodes of the display's clients' ends: the peers of the connections the display's server accepted. Empty when
/// there is no such server or anything can't be read.
pub(super) fn clients(display: &Path, limit: usize) -> HashSet<u64> {
    let listener = listeners(display);
    if listener.is_empty() {
        return HashSet::new();
    }
    let accepted: HashSet<u64> = pids(limit)
        .into_iter()
        .map(|pid| sockets(&pid))
        .filter(|held| !held.is_disjoint(&listener))
        .flat_map(|held| {
            held.into_iter()
                .filter(|inode| !listener.contains(inode))
                .collect::<Vec<_>>()
        })
        .collect();
    let Some(peers) = peers() else {
        return HashSet::new();
    };
    accepted
        .iter()
        .filter_map(|inode| peers.get(inode).copied())
        .collect()
}

/// Whether `pid` holds one of the display's clients' ends.
pub(super) fn holds(pid: i32, clients: &HashSet<u64>) -> bool {
    !clients.is_empty() && !sockets(&pid.to_string()).is_disjoint(clients)
}

/// The display's server's listening sockets, from `/proc/net/unix`.
fn listeners(display: &Path) -> HashSet<u64> {
    let Some(wanted) = display.to_str() else {
        return HashSet::new();
    };
    let Ok(table) = std::fs::read_to_string("/proc/net/unix") else {
        return HashSet::new();
    };
    listening(&table, wanted)
}

/// The inodes of a `/proc/net/unix` table's sockets that listen on `wanted`. A connection the server accepted is
/// listed under the listener's path too, so the listening flag tells them apart.
fn listening(table: &str, wanted: &str) -> HashSet<u64> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            // Num RefCount Protocol Flags Type St Inode Path
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.get(7) != Some(&wanted) {
                return None;
            }
            let flags = u32::from_str_radix(fields.get(3)?, 16).ok()?;
            if flags & ACCEPTING == 0 {
                return None;
            }
            fields.get(6)?.parse().ok()
        })
        .collect()
}

/// The processes to look at, at most `limit`.
fn pids(limit: usize) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        .take(limit)
        .collect()
}

/// The inodes of the sockets a process holds (`socket:[<inode>]` among its descriptors).
fn sockets(pid: &str) -> HashSet<u64> {
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter_map(|target| {
            let target = target.to_str()?;
            target
                .strip_prefix("socket:[")?
                .strip_suffix(']')?
                .parse()
                .ok()
        })
        .collect()
}

/// Each Unix socket's inode and its peer's, from the kernel's socket diagnostics; `None` when they can't be read.
fn peers() -> Option<HashMap<u64, u64>> {
    let netlink = socket(
        AddressFamily::NETLINK,
        SocketType::RAW,
        Some(netlink::SOCK_DIAG),
    )
    .ok()?;
    send(&netlink, &request(), SendFlags::empty()).ok()?;
    let mut found = HashMap::new();
    let mut buffer = vec![0_u8; 1 << 16];
    for _ in 0..MAX_READS {
        let (read, _) = recv(&netlink, &mut buffer[..], RecvFlags::empty()).ok()?;
        if !answers(buffer.get(..read)?, &mut found)? {
            return Some(found);
        }
    }
    None
}

/// A dump of every Unix socket in every state, with its peer.
fn request() -> Vec<u8> {
    let mut message = Vec::with_capacity(HEADER + 24);
    message.extend_from_slice(&40_u32.to_ne_bytes());
    message.extend_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
    message.extend_from_slice(&DUMP.to_ne_bytes());
    message.extend_from_slice(&1_u32.to_ne_bytes());
    message.extend_from_slice(&0_u32.to_ne_bytes());
    // unix_diag_req: family, protocol, padding, every state, any inode, what to show, no cookie.
    message.extend_from_slice(&[UNIX, 0, 0, 0]);
    message.extend_from_slice(&u32::MAX.to_ne_bytes());
    message.extend_from_slice(&0_u32.to_ne_bytes());
    message.extend_from_slice(&SHOW_PEER.to_ne_bytes());
    message.extend_from_slice(&[0; 8]);
    message
}

/// Reads one answer's messages into `found`: `Some(true)` while the dump goes on, `Some(false)` at its end, `None` on
/// an error or a message that doesn't fit.
fn answers(data: &[u8], found: &mut HashMap<u64, u64>) -> Option<bool> {
    let mut offset = 0;
    while offset + HEADER <= data.len() {
        let length = usize::try_from(word(data, offset)?).ok()?;
        let kind = half(data, offset + 4)?;
        if kind == DONE {
            return Some(false);
        }
        if kind == ERROR || length < HEADER || offset + length > data.len() {
            return None;
        }
        let message = data.get(offset + HEADER..offset + length)?;
        if let Some((inode, peer)) = linked(message) {
            found.insert(inode, peer);
        }
        offset += aligned(length);
    }
    Some(true)
}

/// A `unix_diag_msg`'s inode and its `UNIX_DIAG_PEER`, when it has one.
fn linked(message: &[u8]) -> Option<(u64, u64)> {
    let inode = u64::from(word(message, 4)?);
    let mut offset = MESSAGE;
    while offset + 4 <= message.len() {
        let length = usize::from(half(message, offset)?);
        if length < 4 {
            return None;
        }
        if half(message, offset + 2)? == PEER {
            return Some((inode, u64::from(word(message, offset + 4)?)));
        }
        offset += aligned(length);
    }
    None
}

/// Netlink's 4-byte alignment.
const fn aligned(length: usize) -> usize {
    (length + 3) & !3
}

fn word(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn half(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
