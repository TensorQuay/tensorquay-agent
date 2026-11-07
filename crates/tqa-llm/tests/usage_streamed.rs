//! LLM-U-054: the relay's reader counts what a response streamed on every protocol, as the runtime's output meter
//! counts it (text, reasoning, and a call's id, name and argument fragments), so a response cut off before its usage
//! can still be estimated. A thinking block or a call that never ends still counts what arrived.

use serde_json::{Value, json};
use tqa_llm::{
    Protocol,
    relay_shaping::{StreamedBytes, UsageReader},
};

fn streamed(protocol: Protocol, events: &[Value]) -> StreamedBytes {
    let mut reader = UsageReader::new(protocol);
    for event in events {
        reader.observe(&event.to_string());
    }
    reader.streamed()
}

#[test]
fn llm_u_054_chat_counts_text_reasoning_and_call_fragments_and_skips_done() {
    let mut reader = UsageReader::new(Protocol::OpenAiChat);
    for event in [
        json!({"choices":[{"index":0,"delta":{"reasoning_content":"think"}}]}),
        json!({"choices":[{"index":0,"delta":{"content":"hello"}}]}),
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function",
            "function":{"name":"shell","arguments":"{\"a\""}}]}}]}),
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":":1}"}}]}}]}),
    ] {
        reader.observe(&event.to_string());
    }
    reader.observe("[DONE]");
    // think 5 + hello 5 + c1 2 + shell 5 + {"a" 4 + :1} 3
    assert_eq!(
        StreamedBytes {
            output: 24,
            reasoning: 5
        },
        reader.streamed()
    );
    assert_eq!(None, reader.usage(), "no usage was reported");
}

#[test]
fn llm_u_054_responses_counts_its_delta_events_and_a_calls_opening() {
    let events = [
        json!({"type":"response.reasoning_summary_text.delta","delta":"plan"}),
        json!({"type":"response.output_text.delta","delta":"answer"}),
        json!({"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_9","name":"read"}}),
        json!({"type":"response.function_call_arguments.delta","delta":"{\"p\":"}),
        json!({"type":"response.output_item.added","item":{"type":"message"}}),
    ];
    // plan 4 + answer 6 + call_9 6 + read 4 + {"p": 5; the call never ends
    assert_eq!(
        StreamedBytes {
            output: 25,
            reasoning: 4
        },
        streamed(Protocol::OpenAiResponses, &events)
    );
}

#[test]
fn llm_u_054_messages_counts_a_thinking_block_cut_before_it_stops() {
    let events = [
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"long thought"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"edit","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"x\""}}),
    ];
    // long thought 12 + tu_1 4 + edit 4 + {"x" 4; no content_block_stop, no message_delta usage
    assert_eq!(
        StreamedBytes {
            output: 24,
            reasoning: 12
        },
        streamed(Protocol::AnthropicMessages, &events)
    );
}

#[test]
fn llm_u_054_events_that_carry_no_output_add_nothing() {
    for protocol in [
        Protocol::OpenAiChat,
        Protocol::OpenAiResponses,
        Protocol::AnthropicMessages,
    ] {
        let events = [
            json!({"type":"ping"}),
            json!("not an object"),
            json!({"choices":[]}),
        ];
        assert_eq!(
            StreamedBytes::default(),
            streamed(protocol, &events),
            "{protocol:?}"
        );
    }
}
