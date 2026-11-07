//! Chat-completions SSE records: typed deltas, ended by the `[DONE]` sentinel.

use crate::{LlmError, StreamChunk, sse::Wire};

#[cfg(test)]
pub(super) type Decoder = crate::sse::Decoder<ChatWire>;

#[derive(Default)]
pub(super) struct ChatWire {
    done: bool,
    calls: super::wire::CallIndex,
}

impl Wire for ChatWire {
    fn data(&mut self, data: &str) -> Result<Vec<StreamChunk>, LlmError> {
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(Vec::new());
        }
        super::wire::decode_in(data, &mut self.calls)
    }

    fn done(&self) -> bool {
        self.done
    }
}
