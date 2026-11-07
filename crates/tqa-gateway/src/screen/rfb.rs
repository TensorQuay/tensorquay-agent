//! The viewer side of the screen relay (ADR-0039 section 5): every byte a viewer sends is framed and judged here before
//! it reaches the sandbox's VNC server. The relay fails closed: a message type this filter doesn't know, or one whose
//! length doesn't fit, refuses the viewer, and nothing it can't parse passes through. View-only is enforced here, per
//! connection: input (`KeyEvent`, `PointerEvent`, `ClientCutText`) passes only from the viewer holding Take over, and
//! is dropped from every other viewer whatever its page does. Framebuffer update requests pass at most ten a second; a
//! sooner one is held, never dropped, so a viewer's picture never stalls. `SetEncodings` keeps only the encodings that
//! add no client message of their own, so a server that offers more never makes a viewer send what this filter can't
//! frame. The keys and buttons a holder pressed are tracked, so a hand-back or a closed socket releases them.
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

/// The longest paste a viewer may send, in bytes.
const MAX_CUT_TEXT: usize = 256 * 1024;
/// The most encodings a `SetEncodings` may list (noVNC sends about 20).
const MAX_ENCODINGS: usize = 64;
/// Update requests pass at most this often.
const UPDATE_INTERVAL: Duration = Duration::from_millis(100);

/// Whether a viewer may ask for `encoding`: the framebuffer encodings, and the pseudo-encodings the server only sends.
/// Left out are those that add a client message (QEMU's key and audio events, fence, continuous updates, extended
/// desktop size, xvp, GII, the extended clipboard) and anything unknown.
const fn allowed_encoding(encoding: i32) -> bool {
    matches!(
        encoding,
        // Raw, CopyRect, RRE, Hextile, Zlib, Tight, TRLE, ZRLE, JPEG.
        0 | 1 | 2 | 5 | 6 | 7 | 15 | 16 | 21
        // Tight's compression (-256..-247) and JPEG quality (-32..-23) levels, TightPNG.
        | -256..=-247 | -32..=-23 | -260
        // Cursor, X cursor, desktop size, last rect, desktop name, QEMU's LED state, VMware's cursor.
        | -239 | -240 | -223 | -224 | -307 | -261 | 0x574d_5664
    )
}

/// Why a viewer is refused; the relay closes its socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum Refused {
    /// The handshake isn't RFB 3.3, 3.7 or 3.8 with no security, as the sandbox's server offers.
    #[error("the handshake isn't RFB 3.3, 3.7 or 3.8 with no security")]
    Handshake,
    /// A client message type this filter doesn't know.
    #[error("unknown client message type {0}")]
    Unknown(u8),
    /// A message whose length doesn't fit its type's rule, or a paste past the bound.
    #[error("a malformed or oversized client message")]
    Malformed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    /// `RFB 003.00x\n`.
    Version,
    /// The security type the client picks (3.7 and 3.8 only).
    Security,
    /// `ClientInit`: one byte, the shared flag.
    Init,
    Messages,
}

/// One viewer's stream of client messages.
pub(crate) struct ClientFilter {
    phase: Phase,
    pending: Vec<u8>,
    last_update: Option<Instant>,
    held_update: Option<[u8; 10]>,
    /// The keys the holder pressed and hasn't released, as forwarded.
    pressed: BTreeSet<u32>,
    /// The holder's last pointer message: its button mask and position.
    pointer: [u8; 5],
}

impl ClientFilter {
    pub(crate) const fn new() -> Self {
        Self {
            phase: Phase::Version,
            pending: Vec::new(),
            last_update: None,
            held_update: None,
            pressed: BTreeSet::new(),
            pointer: [0; 5],
        }
    }

    /// What lets go of every key and button the holder still has down: a key-up for each pressed key, and a pointer
    /// message with no button at its last position. `None` when nothing is down. Called at a hand-back and when the
    /// socket closes, so nothing stays pressed for the agent's next actions.
    pub(crate) fn release(&mut self) -> Option<Vec<u8>> {
        if self.pressed.is_empty() && self.pointer[0] == 0 {
            return None;
        }
        let mut release = Vec::new();
        for key in std::mem::take(&mut self.pressed) {
            release.extend_from_slice(&[4, 0, 0, 0]);
            release.extend_from_slice(&key.to_be_bytes());
        }
        if self.pointer[0] != 0 {
            self.pointer[0] = 0;
            release.push(5);
            release.extend_from_slice(&self.pointer);
        }
        Some(release)
    }

    /// Frames `bytes` (any split across WebSocket frames) and answers what to forward to the server now. `holder` is
    /// whether this viewer holds Take over.
    ///
    /// # Errors
    /// The viewer is refused: its socket closes.
    pub(crate) fn feed(
        &mut self,
        bytes: &[u8],
        holder: bool,
        now: Instant,
    ) -> Result<Vec<u8>, Refused> {
        self.pending.extend_from_slice(bytes);
        let mut forward = Vec::new();
        while let Some(length) = self.complete()? {
            let message: Vec<u8> = self.pending.drain(..length).collect();
            self.judge(message, holder, now, &mut forward)?;
        }
        Ok(forward)
    }

    /// An update request held back by the pacing, once it's due.
    pub(crate) fn due(&mut self, now: Instant) -> Option<Vec<u8>> {
        let ready = self
            .last_update
            .is_none_or(|last| now.duration_since(last) >= UPDATE_INTERVAL);
        if ready && let Some(request) = self.held_update.take() {
            self.last_update = Some(now);
            return Some(request.to_vec());
        }
        None
    }

    /// When the held update request falls due, if one is held.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.held_update?;
        Some(
            self.last_update
                .map_or_else(Instant::now, |last| last + UPDATE_INTERVAL),
        )
    }

    /// The length of the next complete unit in `pending`, or `None` while it's still incomplete.
    fn complete(&self) -> Result<Option<usize>, Refused> {
        let bytes = &self.pending;
        let Some(&first) = bytes.first() else {
            return Ok(None);
        };
        let need = match self.phase {
            Phase::Version => 12,
            Phase::Security | Phase::Init => 1,
            Phase::Messages => match first {
                0 => 20,
                2 => {
                    let Some(count) = bytes.get(2..4) else {
                        return Ok(None);
                    };
                    let count = usize::from(u16::from_be_bytes([count[0], count[1]]));
                    if count > MAX_ENCODINGS {
                        return Err(Refused::Malformed);
                    }
                    4 + 4 * count
                }
                3 => 10,
                4 => 8,
                5 => 6,
                6 => {
                    let Some(length) = bytes.get(4..8) else {
                        return Ok(None);
                    };
                    // A negative length is the extended clipboard, which this server never offers.
                    let length = i32::from_be_bytes([length[0], length[1], length[2], length[3]]);
                    let length = usize::try_from(length).map_err(|_| Refused::Malformed)?;
                    if length > MAX_CUT_TEXT {
                        return Err(Refused::Malformed);
                    }
                    8 + length
                }
                other => return Err(Refused::Unknown(other)),
            },
        };
        Ok((bytes.len() >= need).then_some(need))
    }

    fn judge(
        &mut self,
        mut message: Vec<u8>,
        holder: bool,
        now: Instant,
        forward: &mut Vec<u8>,
    ) -> Result<(), Refused> {
        match self.phase {
            Phase::Version => {
                // RFB 3.3 has the server pick the security type; 3.7 and 3.8 have the client pick it next.
                self.phase = match message.as_slice() {
                    b"RFB 003.003\n" => Phase::Init,
                    b"RFB 003.007\n" | b"RFB 003.008\n" => Phase::Security,
                    _ => return Err(Refused::Handshake),
                };
                forward.extend_from_slice(&message);
            }
            Phase::Security => {
                // None, the only type the sandbox's server offers (no password, reached only through the bridge).
                if message[0] != 1 {
                    return Err(Refused::Handshake);
                }
                self.phase = Phase::Init;
                forward.extend_from_slice(&message);
            }
            Phase::Init => {
                // Always shared: a viewer asking for an exclusive session would disconnect the others.
                message[0] = 1;
                self.phase = Phase::Messages;
                forward.extend_from_slice(&message);
            }
            Phase::Messages => match message[0] {
                2 => forward.extend_from_slice(&encodings(&message)),
                3 => self.update_request(&message, now, forward),
                4..=6 if !holder => {}
                4 => {
                    let key = u32::from_be_bytes([message[4], message[5], message[6], message[7]]);
                    if message[1] == 0 {
                        self.pressed.remove(&key);
                    } else {
                        self.pressed.insert(key);
                    }
                    forward.extend_from_slice(&message);
                }
                5 => {
                    self.pointer.copy_from_slice(&message[1..6]);
                    forward.extend_from_slice(&message);
                }
                _ => forward.extend_from_slice(&message),
            },
        }
        Ok(())
    }

    /// Passes an update request at most every 100 ms; a sooner one is held (the latest, full if any held was full).
    fn update_request(&mut self, message: &[u8], now: Instant, forward: &mut Vec<u8>) {
        let mut request = [0_u8; 10];
        request.copy_from_slice(message);
        if let Some(held) = self.held_update
            && held[1] == 0
        {
            request[1] = 0;
        }
        if self
            .last_update
            .is_none_or(|last| now.duration_since(last) >= UPDATE_INTERVAL)
        {
            self.held_update = None;
            self.last_update = Some(now);
            forward.extend_from_slice(&request);
        } else {
            self.held_update = Some(request);
        }
    }
}

/// A `SetEncodings` with only the allowed encodings, in the viewer's order.
fn encodings(message: &[u8]) -> Vec<u8> {
    let kept: Vec<[u8; 4]> = message[4..]
        .chunks_exact(4)
        .filter_map(|chunk| <[u8; 4]>::try_from(chunk).ok())
        .filter(|chunk| allowed_encoding(i32::from_be_bytes(*chunk)))
        .collect();
    // At most MAX_ENCODINGS, so the count fits.
    let count = u16::try_from(kept.len()).unwrap_or(u16::MAX);
    let mut filtered = vec![2, 0];
    filtered.extend_from_slice(&count.to_be_bytes());
    for chunk in kept {
        filtered.extend_from_slice(&chunk);
    }
    filtered
}

#[cfg(test)]
#[path = "rfb_tests.rs"]
mod tests;
