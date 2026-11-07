//! Constant-space head/tail collection with incremental UTF-8 decoding per stream.
use std::collections::VecDeque;

#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
    lossy: bool,
}
impl Decoder {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        let mut offset = 0;
        while offset < self.pending.len() {
            let remaining = &self.pending[offset..];
            let error = match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    text.push_str(valid);
                    offset = self.pending.len();
                    break;
                }
                Err(error) => error,
            };
            let end = offset + error.valid_up_to();
            text.push_str(&String::from_utf8_lossy(&self.pending[offset..end]));
            offset = end;
            let Some(length) = error.error_len() else {
                break;
            };
            text.push('\u{fffd}');
            self.lossy = true;
            offset += length;
        }
        self.pending.drain(..offset);
        text
    }
    fn finish(&mut self) -> &'static str {
        if self.pending.is_empty() {
            ""
        } else {
            self.lossy = true;
            "\u{fffd}"
        }
    }
}

pub(super) struct Capture {
    max: usize,
    head: Vec<u8>,
    tail: VecDeque<u8>,
    count: usize,
    stderr_start: bool,
    stdout: Decoder,
    stderr: Decoder,
}
impl Capture {
    pub(super) fn new(max: usize) -> Self {
        Self {
            max,
            head: vec![],
            tail: VecDeque::new(),
            count: 0,
            stderr_start: true,
            stdout: Decoder::default(),
            stderr: Decoder::default(),
        }
    }
    pub(super) fn push(&mut self, bytes: &[u8], stderr: bool) {
        let text = if stderr {
            self.stderr.push(bytes)
        } else {
            self.stdout.push(bytes)
        };
        self.append(&text, stderr);
    }
    fn append(&mut self, text: &str, stderr: bool) {
        for byte in text.bytes() {
            if stderr && self.stderr_start {
                self.extend(b"[stderr] ");
            }
            self.extend(&[byte]);
            if stderr {
                self.stderr_start = byte == b'\n';
            }
        }
    }
    fn extend(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.count = self.count.saturating_add(1);
            if self.head.len() < self.max {
                self.head.push(byte);
                continue;
            }
            if self.tail.len() == self.max {
                self.tail.pop_front();
            }
            self.tail.push_back(byte);
        }
    }
    pub(super) fn finish(mut self) -> String {
        let stdout = self.stdout.finish();
        let stderr = self.stderr.finish();
        self.append(stdout, false);
        self.append(stderr, true);
        let prefix = if self.stdout.lossy || self.stderr.lossy {
            "[lossy UTF-8]\n"
        } else {
            ""
        };
        let tail: Vec<_> = self.tail.into_iter().collect();
        let head_text = String::from_utf8_lossy(&self.head);
        if self.count <= self.max && head_text.len() + prefix.len() <= self.max {
            return format!("{prefix}{head_text}");
        }
        let marker = format!("{prefix}\n[truncated]\n");
        if marker.len() >= self.max {
            return prefix_bytes(&marker, self.max).to_owned();
        }
        let budget = self.max - marker.len();
        let head = prefix_bytes(&head_text, budget.div_ceil(2));
        let tail_source = if tail.is_empty() {
            head_text.clone()
        } else {
            String::from_utf8_lossy(&tail)
        };
        let tail = suffix_bytes(&tail_source, budget / 2);
        format!("{head}{marker}{tail}")
    }
    pub(super) fn finish_with_notice(mut self, notice: &str) -> String {
        let max = self.max;
        if notice.len() >= max {
            return prefix_bytes(notice, max).to_owned();
        }
        self.max = max - notice.len() - 1;
        let text = self.finish();
        let text = text.trim_end_matches('\n');
        if text.is_empty() {
            notice.to_owned()
        } else {
            format!("{text}\n{notice}")
        }
    }
}
fn prefix_bytes(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
fn suffix_bytes(text: &str, bytes: usize) -> &str {
    let mut start = text.len().saturating_sub(bytes);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_u_006_background_notice_keeps_truncation_visible() {
        let notice = "[background processes were stopped when the command returned]";
        for max in [1, 20, 80, 100, 500] {
            let mut capture = Capture::new(max);
            capture.push("é".repeat(1000).as_bytes(), false);
            let output = capture.finish_with_notice(notice);
            assert!(output.len() <= max);
            if max >= 80 {
                assert!(output.contains("[truncated]"));
                assert!(output.ends_with(notice));
            }
        }
    }
    #[test]
    fn tool_e_009_stream_fragments_do_not_corrupt_utf8() {
        let mut capture = Capture::new(64);
        capture.push(&[0xc3], false);
        capture.push(b"warning\n", true);
        capture.push(&[0xa9], false);
        assert_eq!(capture.finish(), "[stderr] warning\né");
    }
    #[test]
    fn tool_e_009_invalid_middle_is_marked_even_after_truncation() {
        let mut capture = Capture::new(64);
        capture.push(&[b'x'; 128], false);
        capture.push(&[0xff], false);
        capture.push(&[b'y'; 128], false);
        let output = capture.finish();
        assert!(output.contains("[lossy UTF-8]"));
        assert!(output.contains("[truncated]"));
        assert!(output.len() <= 64);
    }
    #[test]
    fn tool_u_006_stderr_fragments_and_small_caps() {
        let mut capture = Capture::new(100);
        capture.push(b"one", true);
        capture.push(b"\ntwo", true);
        assert_eq!(capture.finish(), "[stderr] one\n[stderr] two");
        for max in 1..40 {
            let mut capture = Capture::new(max);
            capture.push("é".repeat(100).as_bytes(), false);
            let output = capture.finish();
            assert!(output.len() <= max);
            assert!(!output.contains("lossy"));
        }
    }
}
