//! LLM-C-002/E-006: framing and typed wire messages at every byte boundary.

use super::{sse::Decoder, wire};
use crate::{FinishReason, LlmError, StreamChunk, StreamLimitKind};
use bytes::Bytes;
use serde_json::json;
use std::error::Error;

#[test]
fn llm_e_006_incremental_sse_preserves_multiline_unicode_and_comments() -> Result<(), Box<dyn Error>>
{
    let encoded = b"\r\n: keep-alive\r\nevent: completion\r\nid\r\ndata: {\"choices\":\r\ndata: [{\"delta\":{\"content\":\"\xe7\x95\x8c\xf0\x9f\x99\x82\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n";
    for split in 1..encoded.len() {
        let mut decoder = Decoder::new(1024);
        let mut chunks = Vec::new();
        for bytes in encoded.chunks(split) {
            let mut bytes = Bytes::copy_from_slice(bytes);
            while !bytes.is_empty() && !decoder.done {
                chunks.extend(decoder.decode(&mut bytes)?);
            }
        }
        assert_eq!(chunks, [StreamChunk::TextDelta("界🙂".into())]);
        assert!(decoder.done);
    }
    Ok(())
}

#[test]
fn llm_e_006_invalid_utf8_json_and_oversized_events_are_typed() {
    for invalid in [b"data: \xff\n\n".as_slice(), b"data: {invalid}\n\n"] {
        assert!(matches!(
            Decoder::new(1024).decode(&mut Bytes::copy_from_slice(invalid)),
            Err(LlmError::Malformed(_))
        ));
    }
    assert!(matches!(
        Decoder::new(4).decode(&mut Bytes::from_static(b":1234\n\n")),
        Err(LlmError::StreamLimit {
            which: StreamLimitKind::SseEvent,
            max: 4
        })
    ));
}

#[test]
fn llm_c_002_finish_reasons_and_optional_usage() -> Result<(), Box<dyn Error>> {
    for (value, reason) in [
        ("stop", FinishReason::Stop),
        ("tool_calls", FinishReason::ToolCalls),
        ("length", FinishReason::Length),
        ("content_filter", FinishReason::ContentFilter),
        ("custom", FinishReason::Other("custom".into())),
    ] {
        assert_eq!(
            wire::decode(&json!({"choices":[{"finish_reason":value}]}).to_string())?,
            [StreamChunk::Finish(reason)]
        );
    }
    let chunks = wire::decode(&json!({"choices":[{"delta":{"reasoning":"plan","tool_calls":[{"index":1,"id":"a"}]}}],"usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":5},"completion_tokens_details":{"reasoning_tokens":1}}}).to_string())?;
    assert!(
        matches!(&chunks[..], [StreamChunk::ReasoningDelta(text), StreamChunk::ToolCallDelta { index: 1, id: Some(id), name: None, arguments_delta }, StreamChunk::Usage(usage)] if text == "plan" && id == "a" && arguments_delta.is_empty() && usage.cached_tokens == Some(5) && usage.reasoning_tokens == Some(1))
    );
    Ok(())
}

#[test]
fn llm_e_006_unknown_choices_and_tool_kinds_fail_closed() {
    for value in [
        json!({}),
        json!({"choices":[{"index":1}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"type":"unrecognized"}]}}]}),
    ] {
        assert!(matches!(
            wire::decode(&value.to_string()),
            Err(LlmError::Malformed(_))
        ));
    }
}

/// Decodes a whole Chat stream of `pieces` (each a data-field JSON value) and returns the chunks or the error.
fn stream(pieces: &[serde_json::Value]) -> Result<Vec<StreamChunk>, LlmError> {
    let mut wire = String::new();
    for piece in pieces {
        wire.push_str("data: ");
        wire.push_str(&piece.to_string());
        wire.push_str("\n\n");
    }
    wire.push_str("data: [DONE]\n\n");
    let mut decoder = Decoder::new(1 << 16);
    let mut bytes = Bytes::from(wire);
    let mut chunks = Vec::new();
    while !bytes.is_empty() && !decoder.done {
        chunks.extend(decoder.decode(&mut bytes)?);
    }
    Ok(chunks)
}

fn call(
    id: Option<&str>,
    name: Option<&str>,
    arguments: &str,
    index: Option<u32>,
) -> serde_json::Value {
    let mut call = json!({"type":"function","function":{"arguments":arguments}});
    if let Some(id) = id {
        call["id"] = json!(id);
    }
    if let Some(name) = name {
        call["function"]["name"] = json!(name);
    }
    if let Some(index) = index {
        call["index"] = json!(index);
    }
    json!({"choices":[{"index":0,"delta":{"tool_calls":[call]}}]})
}

#[test]
fn llm_u_042_a_single_tool_call_without_index_is_call_zero() -> Result<(), Box<dyn Error>> {
    // Whole in one piece, as some compatible endpoints stream it, with usage in the same piece.
    let mut whole = call(Some("c1"), Some("read_file"), "{\"path\":\"a\"}", None);
    whole["choices"][0]["finish_reason"] = json!("stop");
    whole["usage"] = json!({"prompt_tokens":7,"completion_tokens":3});
    let chunks = stream(&[whole.clone()])?;
    assert_eq!(
        chunks[0],
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("c1".into()),
            name: Some("read_file".into()),
            arguments_delta: "{\"path\":\"a\"}".into(),
        }
    );
    assert!(
        chunks
            .iter()
            .any(|chunk| matches!(chunk, StreamChunk::Usage(usage) if usage.prompt_tokens == 7))
    );
    // The relay's usage reader, one piece at a time, still reads the usage.
    assert_eq!(
        crate::relay_shaping::read_usage(crate::Protocol::OpenAiChat, &whole.to_string())
            .map(|usage| usage.completion_tokens),
        Some(3)
    );
    // Split: the id and name first, then argument fragments without an id.
    let chunks = stream(&[
        call(Some("c1"), Some("read_file"), "{\"path\"", None),
        call(None, None, ":\"a\"}", None),
    ])?;
    assert!(
        chunks
            .iter()
            .all(|chunk| matches!(chunk, StreamChunk::ToolCallDelta { index: 0, .. })),
        "{chunks:?}"
    );
    // Later pieces that repeat call 0's id continue it, even when they repeat its name too.
    let chunks = stream(&[
        call(Some("c1"), Some("read_file"), "{\"path\"", None),
        call(Some("c1"), Some("read_file"), ":\"a\"}", None),
    ])?;
    assert!(
        chunks
            .iter()
            .all(|chunk| matches!(chunk, StreamChunk::ToolCallDelta { index: 0, .. })),
        "{chunks:?}"
    );
    Ok(())
}

#[test]
fn llm_u_042_several_calls_without_index_are_refused_never_guessed() -> Result<(), Box<dyn Error>> {
    for pieces in [
        // A second call without an index.
        vec![
            call(Some("c1"), Some("a"), "{}", None),
            call(Some("c2"), Some("b"), "{}", None),
        ],
        // Two in one piece.
        vec![json!({"choices":[{"index":0,"delta":{"tool_calls":[
            {"id":"c1","type":"function","function":{"name":"a","arguments":"{}"}},
            {"id":"c2","type":"function","function":{"name":"b","arguments":"{}"}}]}}]})],
        // A later piece without an id but with a name, once call 0 has one: a new call, not more of call 0.
        vec![
            call(Some("c1"), Some("a"), "{}", None),
            call(None, Some("b"), "{}", None),
        ],
        vec![
            call(Some("c1"), Some("a"), "{}", None),
            call(None, Some("a"), "{}", None),
        ],
        // Mixed with indexed calls, either order.
        vec![
            call(Some("c1"), Some("a"), "{}", Some(0)),
            call(Some("c2"), Some("b"), "{}", None),
        ],
        vec![
            call(Some("c1"), Some("a"), "{}", None),
            call(Some("c2"), Some("b"), "{}", Some(1)),
        ],
    ] {
        let result = stream(&pieces);
        let Err(LlmError::Malformed(message)) = &result else {
            return Err(format!("{pieces:?}: {result:?}").into());
        };
        assert!(message.contains("without an index"), "{message}");
    }
    Ok(())
}

/// The `chat.tool_call_extra` item for `call_id` holding `extra` (ADR-0027, Chat tool-call extras).
fn extra_item(call_id: &str, extra: &serde_json::Value) -> StreamChunk {
    StreamChunk::ReasoningState(
        json!({"type":"chat.tool_call_extra","call_id":call_id,"extra_content":extra}).to_string(),
    )
}

fn states(chunks: &[StreamChunk]) -> Vec<StreamChunk> {
    chunks
        .iter()
        .filter(|chunk| matches!(chunk, StreamChunk::ReasoningState(_)))
        .cloned()
        .collect()
}

#[test]
fn llm_u_043_a_tool_calls_extra_content_is_kept_per_call() -> Result<(), Box<dyn Error>> {
    let signed = json!({"google":{"thought_signature":"opaque-1"}});
    let other = json!({"vendor":{"sig":"opaque-2"}});
    // In the call's own piece, without an index.
    let mut whole = call(Some("c1"), Some("read_file"), "{}", None);
    whole["choices"][0]["delta"]["tool_calls"][0]["extra_content"] = signed.clone();
    assert_eq!(states(&stream(&[whole])?), [extra_item("c1", &signed)]);
    // In a later piece than the id, by index; and before the id, held until it comes.
    let mut late = call(None, None, "}", Some(0));
    late["choices"][0]["delta"]["tool_calls"][0]["extra_content"] = signed.clone();
    assert_eq!(
        states(&stream(&[
            call(Some("c1"), Some("a"), "{", Some(0)),
            late.clone()
        ])?),
        [extra_item("c1", &signed)]
    );
    let mut early = call(None, Some("a"), "{", Some(0));
    early["choices"][0]["delta"]["tool_calls"][0]["extra_content"] = signed.clone();
    assert_eq!(
        states(&stream(&[early, call(Some("c1"), None, "}", Some(0))])?),
        [extra_item("c1", &signed)]
    );
    // Parallel calls each keep their own.
    let mut first = call(Some("c1"), Some("a"), "{}", Some(0));
    first["choices"][0]["delta"]["tool_calls"][0]["extra_content"] = signed.clone();
    let mut second = call(Some("c2"), Some("b"), "{}", Some(1));
    second["choices"][0]["delta"]["tool_calls"][0]["extra_content"] = other.clone();
    assert_eq!(
        states(&stream(&[first, second])?),
        [extra_item("c1", &signed), extra_item("c2", &other)]
    );
    // Nothing kept without an extra, or with one that is not an object.
    assert!(states(&stream(&[call(Some("c1"), Some("a"), "{}", None)])?).is_empty());
    let mut odd = call(Some("c1"), Some("a"), "{}", None);
    odd["choices"][0]["delta"]["tool_calls"][0]["extra_content"] = json!("text");
    assert!(states(&stream(&[odd])?).is_empty());
    Ok(())
}
