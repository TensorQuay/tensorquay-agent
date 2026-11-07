//! LLM-C-005: usage mapping on every protocol, with and without the cached and reasoning counters. Present fields are
//! mapped; a counter the server did not report is `None`, never 0.

use serde_json::{Value, json};
use tqa_llm::{Protocol, Usage, relay_shaping::UsageReader};

/// The usage one response reports, observed event by event as the relay reads it.
fn read(protocol: Protocol, events: &[Value]) -> Option<Usage> {
    let mut reader = UsageReader::new(protocol);
    for event in events {
        reader.observe(&event.to_string());
    }
    reader.usage()
}

fn usage(prompt: u64, completion: u64, cached: Option<u64>, reasoning: Option<u64>) -> Usage {
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        cached_tokens: cached,
        reasoning_tokens: reasoning,
    }
}

#[test]
fn llm_c_005_chat_usage_with_and_without_optional_counters() {
    let full = json!({"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7,
        "prompt_tokens_details":{"cached_tokens":4},"completion_tokens_details":{"reasoning_tokens":2}}});
    let bare = json!({"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7}});
    assert_eq!(
        read(Protocol::OpenAiChat, &[full]),
        Some(usage(12, 7, Some(4), Some(2)))
    );
    assert_eq!(
        read(Protocol::OpenAiChat, &[bare]),
        Some(usage(12, 7, None, None))
    );
}

#[test]
fn llm_c_005_responses_usage_with_and_without_optional_counters() {
    let terminal = |usage: Value| json!({"type":"response.completed","response":{"usage":usage}});
    let full = terminal(json!({"input_tokens":12,"output_tokens":7,
        "input_tokens_details":{"cached_tokens":4},"output_tokens_details":{"reasoning_tokens":2}}));
    let bare = terminal(json!({"input_tokens":12,"output_tokens":7}));
    assert_eq!(
        read(Protocol::OpenAiResponses, &[full]),
        Some(usage(12, 7, Some(4), Some(2)))
    );
    assert_eq!(
        read(Protocol::OpenAiResponses, &[bare]),
        Some(usage(12, 7, None, None))
    );
}

#[test]
fn llm_c_005_messages_usage_with_and_without_cache_counters() {
    let start = |usage: Value| json!({"type":"message_start","message":{"usage":usage}});
    let end = json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}});
    let cached = start(json!({"input_tokens":8,"cache_read_input_tokens":4,"output_tokens":0}));
    let bare = start(json!({"input_tokens":12,"output_tokens":0}));
    // The protocol reports no reasoning count, so it is always `None`; the prompt is the whole prompt.
    assert_eq!(
        read(Protocol::AnthropicMessages, &[cached, end.clone()]),
        Some(usage(12, 7, Some(4), None))
    );
    assert_eq!(
        read(Protocol::AnthropicMessages, &[bare, end]),
        Some(usage(12, 7, None, None))
    );
}
