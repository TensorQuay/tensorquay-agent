//! PROTO-C-001: reproducible 1000-case wire round-trip properties.

use std::{fmt::Debug, str::FromStr};

use proptest::{prelude::*, test_runner::RngSeed};
use serde_json::json;
use tqa_types::{
    ClientFrame, ConversationId, ErrorCode, Event, RunId, SandboxBackend, SandboxState,
    SandboxSummary, Seq, ServerFrame, ToolCallId, ToolStatus,
};

fn id<T: FromStr + Debug + 'static>() -> impl Strategy<Value = T> {
    "[A-Za-z0-9_-]{1,16}".prop_filter_map("validated identifier", |text| text.parse().ok())
}

/// A tool result, with a computer call's `target` half the time (revision 14, PROTO-U-037).
fn tool_result(call_id: ToolCallId, text: String, with_target: bool) -> Event {
    let target = with_target
        .then(|| {
            tqa_types::ScreenTarget::new(
                tqa_types::ScreenRole::Button,
                text.lines().next().unwrap_or("").to_owned(),
            )
            .ok()
        })
        .flatten();
    Event::ToolResult {
        call_id,
        status: ToolStatus::Ok,
        content: text,
        target,
        evidence: None,
    }
}

/// A `usage` event, with or without the optional counts and the request's estimates.
fn usage(flag: bool, tokens: u64) -> Event {
    Event::Usage {
        estimated: flag,
        prompt_tokens: tokens,
        completion_tokens: tokens,
        cached_tokens: flag.then_some(tokens),
        reasoning_tokens: None,
        estimated_input: flag.then_some(tokens),
        estimated_images: flag.then_some(tokens),
        estimated_route: flag.then(|| "local".into()),
    }
}

fn events() -> impl Strategy<Value = Event> {
    (
        0_u8..18,
        id::<RunId>(),
        id::<ToolCallId>(),
        ".{0,48}",
        any::<u32>(),
        any::<u64>(),
        any::<bool>(),
    )
        .prop_map(
            |(kind, run_id, call_id, text, number, tokens, flag)| match kind {
                0 => Event::RunStarted { run_id },
                1 => Event::StepStarted {
                    step: number,
                    attempt: number,
                },
                2 => Event::TextDelta { text },
                3 => Event::ReasoningDelta { text },
                4 => Event::ToolCall {
                    call_id,
                    name: "fixture".to_owned(),
                    arguments: json!({"value": text, "tokens": tokens}),
                },
                5 => Event::ToolStart { call_id },
                6 => Event::ToolOutput {
                    call_id,
                    chunk: number,
                    last: flag,
                    data: text,
                },
                7 => Event::ToolEnd {
                    call_id,
                    status: ToolStatus::Ok,
                    exit_code: flag.then_some(0),
                    failure: None,
                },
                8 => usage(flag, tokens),
                9 => Event::RunCompleted { run_id },
                10 => Event::RunCancelled { run_id },
                11 => Event::RunFailed {
                    run_id,
                    reason: text,
                    code: ErrorCode::Budget,
                    provider_message: None,
                },
                12 => Event::RunResumed {
                    run_id,
                    from_step: number,
                },
                14 => Event::MessageUser {
                    lead_brief: None,
                    team_wake: None,
                    text: format!("message{text}"),
                    attachments: vec![],
                },
                15 => tool_result(call_id, text, flag),
                16 => Event::StepCompleted {
                    step: number,
                    duration_ms: 0,
                },
                17 => Event::ModelCompleted {
                    step: number,
                    attempt: number,
                    finish_reason: finish_reason(number),
                },
                _ => Event::SandboxState {
                    state: SandboxState::Ready,
                },
            },
        )
}

fn finish_reason(number: u32) -> tqa_types::FinishReason {
    match number % 5 {
        0 => tqa_types::FinishReason::Stop,
        1 => tqa_types::FinishReason::ToolCalls,
        2 => tqa_types::FinishReason::Length,
        3 => tqa_types::FinishReason::ContentFilter,
        _ => tqa_types::FinishReason::Other,
    }
}

fn clients() -> impl Strategy<Value = ClientFrame> {
    (
        0_u8..3,
        id::<ConversationId>(),
        id::<RunId>(),
        ".{1,48}",
        any::<u64>(),
        any::<u16>(),
        prop::collection::btree_set("uploads/[A-Za-z0-9_-][A-Za-z0-9._-]{0,15}", 0..5)
            .prop_map(|paths| paths.into_iter().collect()),
    )
        .prop_map(
            |(kind, conversation_id, run_id, text, seq, protocol, attachments)| match kind {
                0 => ClientFrame::SessionInit {
                    conversation_id,
                    last_seq: Seq::from(seq),
                    protocol,
                },
                1 => ClientFrame::UserMessage {
                    lead_brief: None,
                    team_wake: None,
                    text,
                    attachments,
                    host_keys: Vec::new(),
                },
                _ => ClientFrame::RunCancel { run_id },
            },
        )
}

fn servers() -> impl Strategy<Value = ServerFrame> {
    (
        0_u8..5,
        id::<ConversationId>(),
        id::<RunId>(),
        events(),
        any::<u64>(),
    )
        .prop_map(|(kind, conversation_id, run_id, event, seq)| match kind {
            0 => ServerFrame::SessionReady {
                queue: None,
                conversation_id,
                resumed: true,
                sandbox: SandboxSummary::new(SandboxBackend::Docker, SandboxState::Ready),
            },
            1 => ServerFrame::Event {
                seq: Seq::from(seq),
                run_id: Some(run_id),
                step: None,
                ts: "2026-09-22T00:00:00Z".to_owned(),
                event,
            },
            2 => ServerFrame::PermissionRequest {
                seq: Seq::from(seq),
            },
            4 => ServerFrame::Heartbeat { run_id },
            _ => ServerFrame::Error {
                target: None,
                seq: None,
                code: ErrorCode::InvalidFrame,
                message: "fixture".to_owned(),
                retryable: false,
            },
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        rng_seed: RngSeed::Fixed(3001),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn proto_c_001_property_event_round_trip(event in events()) {
        let encoded = serde_json::to_vec(&event)?;
        let decoded: Event = serde_json::from_slice(&encoded)?;
        prop_assert_eq!(decoded, event);
    }

    #[test]
    fn proto_c_001_property_client_round_trip(frame in clients()) {
        let encoded = serde_json::to_vec(&frame)?;
        prop_assert_eq!(ClientFrame::parse(&encoded)?, frame);
    }

    #[test]
    fn proto_c_001_property_server_round_trip(frame in servers()) {
        let encoded = serde_json::to_vec(&frame)?;
        prop_assert_eq!(ServerFrame::parse(&encoded)?, frame);
    }
}
