//! CORE-U-084 with CORE-U-044: older replayed states go at the route's density, one by one, and no more than the
//! limit needs (found on 7 Oct: subtracting them unscaled from a scaled total dropped about the density's multiple too
//! many).
use super::{sendable, states_tokens};
use tqa_context::{Message, ReasoningStateRecord, Role};
use tqa_types::{CacheHint, RouteSnapshot, WireProtocol};

fn route() -> RouteSnapshot {
    RouteSnapshot {
        name: "test".into(),
        protocol: WireProtocol::OpenAiResponses,
        model: "fake-model".into(),
        relay_path: "/relay/r/test".into(),
        context_window: Some(32768),
        max_output: Some(4096),
        native_tools: true,
        parallel_tools: true,
        vision: false,
        reasoning: None,
        max_request_bytes: None,
        cache_hint: CacheHint::None,
        first_response_secs: None,
        pixel_grounding: false,
    }
}

/// An assistant message carrying one replayable state of a fixed size.
fn stateful(index: usize) -> Message {
    let mut message = Message::new(Role::Assistant, format!("step {index}"));
    message.reasoning_states.push(ReasoningStateRecord {
        protocol: WireProtocol::OpenAiResponses,
        route: "test".into(),
        model: "fake-model".into(),
        item: Some(format!(
            r#"{{"type":"reasoning","id":"rs_{index}","encrypted_content":"{}"}}"#,
            "x".repeat(700)
        )),
    });
    message
}

// TEST: CORE-U-084
// TEST: CORE-U-044
#[test]
fn core_u_084_older_states_go_at_the_density_and_no_more_than_needed() {
    // Tenths, to scale as the measure does.
    let density = |tokens: u64| tokens * 16 / 10;
    let history: Vec<Message> = (0..7).map(stateful).collect();
    let state = states_tokens(&history[0]);
    let needed = |messages: &[Message]| {
        density(
            messages
                .iter()
                .map(|message| u64::from(tqa_context::message_tokens(message).tokens))
                .sum(),
        )
    };
    // One and a half states over the limit, at the density: two must go.
    let limit = needed(&history) - density(state) * 3 / 2;
    let (sent, kept) = sendable(history, &route(), limit, needed, density);
    let dropped = sent
        .iter()
        .filter(|message| message.reasoning_states.is_empty())
        .count();
    assert_eq!(
        dropped, 2,
        "the fewest states that bring it under the limit"
    );
    assert!(needed(&sent) <= limit);
    assert!(
        !sent[6].reasoning_states.is_empty() && kept == state,
        "the latest message's states always stay"
    );
}
