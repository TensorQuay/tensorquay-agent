//! UTF-8 output chunking and bounded, ordered reassembly.

use std::collections::HashMap;

use crate::{Event, TOOL_OUTPUT_CHUNK_BYTES, ToolCallId};

/// Splits output into bounded UTF-8 chunks, including one final chunk for empty output.
#[must_use]
pub fn chunk_tool_output(call_id: ToolCallId, data: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut remaining = data;
    let mut chunk = 0_u32;
    loop {
        let mut end = remaining.len().min(TOOL_OUTPUT_CHUNK_BYTES);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        let part = remaining[..end].to_owned();
        if end == remaining.len() {
            events.push(Event::ToolOutput {
                call_id,
                chunk,
                last: true,
                data: part,
            });
            return events;
        }
        events.push(Event::ToolOutput {
            call_id: call_id.clone(),
            chunk,
            last: false,
            data: part,
        });
        remaining = &remaining[end..];
        chunk = chunk.saturating_add(1);
    }
}

/// A rejected chunk; reassembly state is unchanged for every error.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ChunkError {
    /// The event is not a tool-output chunk.
    #[error("expected a tool.output event")]
    NotToolOutput,
    /// The chunk does not follow the last accepted index.
    #[error("expected chunk {expected}, got {got}")]
    OutOfOrder {
        /// The next expected chunk index.
        expected: u32,
        /// The received chunk index.
        got: u32,
    },
    /// A chunk exceeds the wire chunk-size limit.
    #[error("chunk has {bytes} bytes, maximum {max}")]
    ChunkTooLarge {
        /// The received chunk size.
        bytes: usize,
        /// The allowed chunk size.
        max: usize,
    },
    /// The accumulated output exceeds the caller's per-call limit.
    #[error("output would have {bytes} bytes, maximum {max}")]
    OutputTooLarge {
        /// The proposed accumulated byte count.
        bytes: usize,
        /// The configured byte limit.
        max: usize,
    },
    /// Starting this call would exceed the concurrent-call limit.
    #[error("at most {max} tool calls may be assembled concurrently")]
    TooManyCalls {
        /// The configured concurrent-call limit.
        max: usize,
    },
    /// A nonzero chunk arrived for a call without an accepted first chunk.
    #[error("tool call has not started with chunk zero")]
    UnknownCall,
}

#[derive(Debug)]
struct PendingOutput {
    next: Option<u32>,
    data: String,
}

/// Reassembles interleaved calls with explicit concurrency and byte caps.
#[derive(Debug)]
pub struct ToolOutputAssembler {
    max_in_flight_calls: usize,
    max_output_bytes_per_call: usize,
    pending: HashMap<ToolCallId, PendingOutput>,
}

impl ToolOutputAssembler {
    /// Uses exactly the supplied limits; recommendations are never applied implicitly.
    #[must_use]
    pub fn new(max_in_flight_calls: usize, max_output_bytes_per_call: usize) -> Self {
        Self {
            max_in_flight_calls,
            max_output_bytes_per_call,
            pending: HashMap::new(),
        }
    }

    /// Accepts an ordered output chunk and returns completed output only on the final chunk.
    ///
    /// # Errors
    /// Rejects other events, invalid ordering and capacity violations without changing buffered state.
    pub fn push(&mut self, event: &Event) -> Result<Option<String>, ChunkError> {
        let Event::ToolOutput {
            call_id,
            chunk,
            last,
            data,
        } = event
        else {
            return Err(ChunkError::NotToolOutput);
        };
        if data.len() > TOOL_OUTPUT_CHUNK_BYTES {
            return Err(ChunkError::ChunkTooLarge {
                bytes: data.len(),
                max: TOOL_OUTPUT_CHUNK_BYTES,
            });
        }
        let pending = self.pending.get(call_id);
        if let Some(pending) = pending {
            if pending.next != Some(*chunk) {
                return Err(ChunkError::OutOfOrder {
                    expected: pending.next.unwrap_or(u32::MAX),
                    got: *chunk,
                });
            }
        } else {
            if *chunk != 0 {
                return Err(ChunkError::UnknownCall);
            }
            if self.pending.len() >= self.max_in_flight_calls {
                return Err(ChunkError::TooManyCalls {
                    max: self.max_in_flight_calls,
                });
            }
        }
        let bytes = pending
            .map_or(0, |output| output.data.len())
            .saturating_add(data.len());
        if bytes > self.max_output_bytes_per_call {
            return Err(ChunkError::OutputTooLarge {
                bytes,
                max: self.max_output_bytes_per_call,
            });
        }
        if *last {
            let mut output = self
                .pending
                .remove(call_id)
                .map_or_else(String::new, |output| output.data);
            output.push_str(data);
            return Ok(Some(output));
        }
        let output = self
            .pending
            .entry(call_id.clone())
            .or_insert_with(|| PendingOutput {
                next: None,
                data: String::new(),
            });
        output.data.push_str(data);
        output.next = chunk.checked_add(1);
        Ok(None)
    }
}
