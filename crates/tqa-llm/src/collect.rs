//! Bounded collection shared by callers and transport stream-cut diagnostics.

use crate::{
    Completion, CompletionStream, FinishReason, LlmError, StreamChunk, StreamLimitKind,
    StreamLimits, ToolCall,
};
use futures::StreamExt;
use std::collections::BTreeMap;

/// A bounded stream assembly shared by transports, strict collection and the agent loop.
/// Callers discard the assembler after a rejected chunk.
pub struct StreamAssembler {
    completion: Completion,
    calls: BTreeMap<u32, ToolCall>,
    bytes: usize,
    limits: StreamLimits,
}

pub(crate) type Accumulator = StreamAssembler;

impl StreamAssembler {
    /// Creates an empty assembly with explicit memory and call-count bounds.
    #[must_use]
    pub fn new(limits: StreamLimits) -> Self {
        Self {
            completion: Completion::default(),
            calls: BTreeMap::new(),
            bytes: 0,
            limits,
        }
    }

    fn reserve(&mut self, bytes: usize) -> Result<(), LlmError> {
        self.bytes = self.bytes.checked_add(bytes).ok_or(LlmError::StreamLimit {
            which: StreamLimitKind::Completion,
            max: self.limits.max_completion_bytes,
        })?;
        if self.bytes > self.limits.max_completion_bytes {
            return Err(LlmError::StreamLimit {
                which: StreamLimitKind::Completion,
                max: self.limits.max_completion_bytes,
            });
        }
        Ok(())
    }

    /// Appends a delta while enforcing the shared stream bounds and stable call identity.
    ///
    /// # Errors
    /// Rejects size limits and a changed call identifier or name.
    pub fn push(&mut self, chunk: &StreamChunk) -> Result<(), LlmError> {
        match chunk {
            StreamChunk::TextDelta(text) => {
                self.reserve(text.len())?;
                self.completion.text.push_str(text);
            }
            StreamChunk::ReasoningDelta(text) => {
                self.reserve(text.len())?;
                self.completion.reasoning.push_str(text);
            }
            StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => self.push_call(*index, id.as_deref(), name.as_deref(), arguments_delta)?,
            StreamChunk::ReasoningState(item) => {
                self.reserve(item.len())?;
                self.completion.reasoning_state.push(item.clone());
            }
            StreamChunk::Usage(usage) => self.completion.usage = Some(usage.clone()),
            StreamChunk::Finish(reason) => self.completion.finish_reason = Some(reason.clone()),
            StreamChunk::Withheld => {
                self.completion.withheld_chunks = self.completion.withheld_chunks.saturating_add(1);
            }
        }
        Ok(())
    }

    fn push_call(
        &mut self,
        index: u32,
        id: Option<&str>,
        name: Option<&str>,
        arguments: &str,
    ) -> Result<(), LlmError> {
        if !self.calls.contains_key(&index) && self.calls.len() >= self.limits.max_tool_calls {
            return Err(LlmError::StreamLimit {
                which: StreamLimitKind::ToolCalls,
                max: self.limits.max_tool_calls,
            });
        }
        self.reserve(
            arguments
                .len()
                .saturating_add(id.map_or(0, str::len))
                .saturating_add(name.map_or(0, str::len)),
        )?;
        let call = self.calls.entry(index).or_insert_with(|| ToolCall {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
        set_call_field(&mut call.id, id, "identifier")?;
        set_call_field(&mut call.name, name, "name")?;
        call.arguments.push_str(arguments);
        Ok(())
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.completion.finish_reason.is_some()
    }

    /// Copies the current partial response, with calls ordered by their stream index.
    #[must_use]
    pub fn snapshot(&self) -> Completion {
        let mut completion = self.completion.clone();
        completion.tool_calls = self.calls.values().cloned().collect();
        completion
    }

    /// Finishes assembly while preserving raw tool arguments for corrective feedback.
    ///
    /// # Errors
    /// Rejects a missing finish reason, identifier or tool name.
    pub fn finish(mut self) -> Result<Completion, LlmError> {
        if !self.is_finished() {
            return Err(LlmError::StreamCut {
                partial: Box::new(self.snapshot()),
            });
        }
        for call in self.calls.values() {
            if call.id.is_empty() || call.name.is_empty() {
                return Err(LlmError::Malformed(
                    "tool call is missing its identifier or name".into(),
                ));
            }
        }
        self.completion.tool_calls = self.calls.into_values().collect();
        Ok(self.completion)
    }
}

fn set_call_field(target: &mut String, value: Option<&str>, field: &str) -> Result<(), LlmError> {
    if let Some(value) = value {
        if !target.is_empty() && target != value {
            return Err(LlmError::Malformed(format!(
                "tool {field} changed within a call"
            )));
        }
        value.clone_into(target);
    }
    Ok(())
}

/// Collects one stream under caller-supplied limits, preserving tool-call indices.
///
/// # Errors
/// Propagates provider errors and rejects cuts, oversized output or malformed final tool arguments.
pub async fn collect(
    mut stream: CompletionStream,
    limits: StreamLimits,
) -> Result<Completion, LlmError> {
    let mut accumulator = Accumulator::new(limits);
    while let Some(chunk) = stream.next().await {
        accumulator.push(&chunk?)?;
    }
    if !accumulator.is_finished() {
        return accumulator.finish();
    }
    let cut = accumulator.completion.finish_reason == Some(FinishReason::Length);
    for (index, call) in &accumulator.calls {
        serde_json::from_str::<serde_json::Value>(&call.arguments).map_err(|error| {
            if cut {
                LlmError::ToolArgsCut {
                    index: *index,
                    output_tokens: accumulator
                        .completion
                        .usage
                        .as_ref()
                        .map(|usage| usage.completion_tokens),
                }
            } else {
                LlmError::ToolArgsInvalid {
                    index: *index,
                    detail: crate::redact(&error.to_string()),
                }
            }
        })?;
    }
    accumulator.finish()
}
