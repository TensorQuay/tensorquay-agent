use super::tap::UsageTap;
use tqa_llm::Protocol;

#[test]
fn gw_i_039_usage_tap_handles_split_utf8_crlf_comments_and_multiline_events() {
    let mut tap = UsageTap::new(Protocol::OpenAiChat, 1024);
    let bytes = ": keep-alive\r\ndata: {\"choices\":[],\r\ndata: \"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7}}\r\n\r\n".as_bytes();
    for byte in bytes {
        tap.push(&[*byte]);
    }
    let usage = tap.latest();
    assert_eq!(
        usage.map(|usage| (usage.prompt_tokens, usage.completion_tokens)),
        Some((12, 7))
    );
    tap.push(b"data: [DONE]\n\n");
    assert_eq!(tap.latest().map(|usage| usage.prompt_tokens), Some(12));
}
#[test]
fn gw_i_039_oversized_or_invalid_usage_does_not_stop_response_forwarding() {
    let mut tap = UsageTap::new(Protocol::OpenAiChat, 128);
    tap.push(&[b'x'; 129]);
    tap.push(b"\n\ndata: invalid\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2}}\n\n");
    assert_eq!(tap.latest().map(|usage| usage.completion_tokens), Some(2));
}

#[test]
fn gw_i_039_usage_accepts_sse_bom_and_all_line_endings() {
    for ending in ["\r", "\n", "\r\n"] {
        let mut tap = UsageTap::new(Protocol::OpenAiChat, 128);
        let bytes = format!(
            "\u{feff}data: {{\"choices\":[],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":2}}}}{ending}{ending}"
        );
        for byte in bytes.bytes() {
            tap.push(&[byte]);
        }
        assert_eq!(tap.latest().map(|usage| usage.prompt_tokens), Some(1));
    }
}

#[test]
fn gw_i_039_messages_usage_split_across_events_is_combined() {
    let mut tap = UsageTap::new(Protocol::AnthropicMessages, 1024);
    let start = r#"{"type":"message_start","message":{"usage":{"input_tokens":8,"cache_read_input_tokens":4,"output_tokens":0}}}"#;
    let delta = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}"#;
    let body = format!(
        "event: message_start\ndata: {start}\n\nevent: ping\ndata: {{\"type\":\"ping\"}}\n\nevent: message_delta\ndata: {delta}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    );
    for byte in body.bytes() {
        tap.push(&[byte]);
    }
    let usage = tap.latest().map(|usage| {
        (
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.cached_tokens,
            usage.reasoning_tokens,
        )
    });
    assert_eq!(usage, Some((12, 7, Some(4), None)));
}
