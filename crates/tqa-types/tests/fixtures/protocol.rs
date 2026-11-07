//! Representative protocol values, including every enum variant and boundary cases.

use std::error::Error;

use serde_json::json;
use tqa_types::{
    ClientFrame, ErrorCode, Event, MAX_USER_MESSAGE_BYTES, SandboxBackend, SandboxState,
    SandboxSummary, Seq, ServerFrame, ToolStatus,
};

pub fn clients() -> Result<Vec<ClientFrame>, Box<dyn Error>> {
    let frames = vec![
        ClientFrame::SessionInit {
            conversation_id: "c".parse()?,
            last_seq: Seq::ZERO,
            protocol: 1,
        },
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            text: "hello".to_owned(),
            attachments: vec!["uploads/attachment_1".to_owned()],
            host_keys: Vec::new(),
        },
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            text: "x".repeat(MAX_USER_MESSAGE_BYTES),
            attachments: vec![],
            host_keys: Vec::new(),
        },
        ClientFrame::RunCancel {
            run_id: "r".parse()?,
        },
    ];
    Ok(frames)
}

pub fn events() -> Result<Vec<Event>, Box<dyn Error>> {
    let run_id: tqa_types::RunId = "r".parse()?;
    let call_id: tqa_types::ToolCallId = "call".parse()?;
    let mut events = vec![
        Event::EnvironmentChanged {
            template: "workstation".into(),
            capabilities: vec![tqa_types::Capability::Shell, tqa_types::Capability::Browser],
            version: None,
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
        Event::StepStarted {
            step: u32::MAX,
            attempt: 1,
        },
        Event::TextDelta {
            text: "hello".to_owned(),
        },
        Event::ReasoningDelta {
            text: "reason".to_owned(),
        },
        Event::ToolCall {
            call_id: call_id.clone(),
            name: "read_file".to_owned(),
            arguments: json!({"path": "README.md"}),
        },
        Event::ToolStart {
            call_id: call_id.clone(),
        },
        Event::ToolOutput {
            call_id: call_id.clone(),
            chunk: 0,
            last: true,
            data: String::new(),
        },
        usage(u64::MAX, Some(u64::MAX)),
        usage(0, None),
        Event::RunCompleted {
            run_id: run_id.clone(),
        },
        Event::RunCancelled {
            run_id: run_id.clone(),
        },
        Event::RunFailed {
            run_id: run_id.clone(),
            reason: "limit reached".to_owned(),
            code: ErrorCode::Budget,
            provider_message: None,
        },
        Event::RunResumed {
            run_id,
            from_step: u32::MAX,
        },
        // Retired by ADR-0035: still parsed from stored histories, never sent.
        Event::PermissionResolved {},
        Event::PermissionExpired {},
        Event::RunParked {},
    ];
    for (status, exit_code) in [
        (ToolStatus::Ok, Some(i32::MIN)),
        (ToolStatus::Error, Some(i32::MIN)),
        (ToolStatus::Cancelled, Some(i32::MIN)),
        (ToolStatus::Interrupted, Some(i32::MIN)),
        (ToolStatus::Ok, None),
    ] {
        events.push(Event::ToolEnd {
            call_id: call_id.clone(),
            status,
            exit_code,
            failure: None,
        });
    }
    for state in states() {
        events.push(Event::SandboxState { state });
    }
    events.extend(context_events()?);
    events.push(screen_capture()?);
    Ok(events)
}

fn usage(tokens: u64, optional_tokens: Option<u64>) -> Event {
    Event::Usage {
        estimated: false,
        prompt_tokens: tokens,
        completion_tokens: tokens,
        cached_tokens: optional_tokens,
        reasoning_tokens: optional_tokens,
        estimated_input: optional_tokens,
        estimated_images: optional_tokens,
        estimated_route: optional_tokens.map(|_| "local".into()),
    }
}

fn states() -> [SandboxState; 8] {
    [
        SandboxState::Creating,
        SandboxState::Ready,
        SandboxState::Busy,
        SandboxState::Idle,
        SandboxState::Queued,
        SandboxState::Parked,
        SandboxState::Destroyed,
        SandboxState::Failed,
    ]
}

pub fn servers() -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let mut frames = Vec::new();
    for backend in [SandboxBackend::Docker, SandboxBackend::Firecracker] {
        for state in states() {
            frames.push(ServerFrame::SessionReady {
                conversation_id: "c".parse()?,
                resumed: false,
                sandbox: SandboxSummary::new(backend, state),
                queue: (state == SandboxState::Queued)
                    .then(|| tqa_types::QueuePosition::new(0))
                    .transpose()?,
            });
        }
    }
    for event in events()? {
        frames.push(ServerFrame::Event {
            seq: Seq::from(u64::MAX),
            run_id: Some("r".parse()?),
            step: Some(u32::MAX),
            ts: "2026-09-22T00:00:00Z".to_owned(),
            event,
        });
    }
    frames.push(ServerFrame::Event {
        seq: Seq::ZERO,
        run_id: None,
        step: None,
        ts: "2026-09-22T01:02:03.456+08:00".to_owned(),
        event: Event::TextDelta {
            text: String::new(),
        },
    });
    // Retired by ADR-0035: still parsed from stored histories, never sent.
    frames.push(ServerFrame::PermissionRequest { seq: Seq::ZERO });
    for code in error_codes() {
        frames.push(ServerFrame::Error {
            target: None,
            seq: None,
            code,
            message: "fixture error".to_owned(),
            retryable: false,
        });
        frames.push(ServerFrame::Error {
            target: None,
            seq: Some(Seq::ZERO),
            code,
            message: String::new(),
            retryable: true,
        });
    }
    frames.push(ServerFrame::Heartbeat {
        run_id: "r".parse()?,
    });
    frames.push(ServerFrame::TaskTitled {
        title: "Sales CSV cleanup".to_owned(),
    });
    frames.push(ServerFrame::TeamChanged {
        team: "0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b".to_owned(),
        version: 9,
        last_seq: 15,
    });
    frames.extend(screen_frames());
    Ok(frames)
}

/// The Desktop's frames (revision 14, ADR-0039).
fn screen_frames() -> Vec<ServerFrame> {
    vec![
        ServerFrame::ScreenNotice {
            kind: tqa_types::ScreenNoticeKind::HandedBack,
            by: tqa_types::ScreenPerson {
                id: "0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6c".to_owned(),
                name: "Ada".to_owned(),
            },
            at: "2026-10-05T12:00:00Z".to_owned(),
            after_seq: 41,
            seconds: Some(95),
            ended: Some(tqa_types::ScreenEnded::SocketClosed),
            reason: None,
        },
        ServerFrame::ScreenUpload {
            id: "a".repeat(64),
            chunk: 0,
            last: true,
            data: "iVBORw0KGgo=".to_owned(),
        },
        ServerFrame::ScreenSignOutDone {
            ok: false,
            reason: Some(tqa_types::SignOutFailure::ProfileInUse),
            ready: true,
        },
    ]
}

fn error_codes() -> [ErrorCode; 21] {
    [
        ErrorCode::UnknownFrame,
        ErrorCode::InvalidFrame,
        ErrorCode::SeqGap,
        ErrorCode::UnsupportedProtocol,
        ErrorCode::Busy,
        ErrorCode::PermissionBinding,
        ErrorCode::EgressDenied,
        ErrorCode::Restarting,
        ErrorCode::StoreUnavailable,
        ErrorCode::ProviderError,
        ErrorCode::Budget,
        ErrorCode::RepeatedFailure,
        ErrorCode::NoProgress,
        ErrorCode::Io,
        ErrorCode::Internal,
        ErrorCode::SandboxFailed,
        ErrorCode::RuntimeStartFailed,
        ErrorCode::ActionFailed,
        ErrorCode::WorkspaceQuota,
        ErrorCode::StateQuota,
        ErrorCode::ReplyCut,
    ]
}

fn context_events() -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "hello".into(),
            attachments: vec!["uploads/attachment_1".into()],
        },
        Event::StepCompleted {
            step: u32::MAX,
            duration_ms: 0,
        },
        Event::ToolResult {
            call_id: "call".parse()?,
            status: ToolStatus::Ok,
            content: "result".into(),
            target: None,
            evidence: None,
        },
        Event::WorkspaceFull {
            call_id: "call".parse()?,
        },
    ];
    for finish_reason in finish_reasons() {
        events.push(Event::ModelCompleted {
            step: 1,
            attempt: 2,
            finish_reason,
        });
    }
    Ok(events)
}

fn finish_reasons() -> [tqa_types::FinishReason; 5] {
    use tqa_types::FinishReason::{ContentFilter, Length, Other, Stop, ToolCalls};
    [Stop, ToolCalls, Length, ContentFilter, Other]
}

/// A `computer` call's stored screenshot (revision 14), at its bounds.
fn screen_capture() -> Result<Event, Box<dyn Error>> {
    Ok(Event::ScreenCapture {
        call_id: "c-capture".parse()?,
        phase: tqa_types::CapturePhase::Before,
        id: "shot-1".to_owned(),
        width: 1280,
        height: 800,
        bytes: 524_288,
        sha256: "0".repeat(64),
        sent_to_model: true,
        marked: true,
    })
}
