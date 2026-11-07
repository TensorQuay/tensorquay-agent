//! PROTO-U-027: a history stored before ADR-0035 still loads. Each line of the fixture was written by the types as
//! they were before the removal (the approval request, its resolution and expiry, parking, the parked state, a policy
//! denial on a tool result, and the binding error code); each still parses, keeps its sequence, and reads back as a
//! retired marker that holds nothing else.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ClientFrame, ErrorCode, Event, SandboxState, Seq, ServerFrame};

const HISTORY: &str = include_str!("fixtures/retired/pre-adr-0035.jsonl");

fn history() -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    Ok(HISTORY
        .lines()
        .map(|line| ServerFrame::parse(line.as_bytes()))
        .collect::<Result<_, _>>()?)
}

#[test]
fn proto_u_027_every_stored_frame_parses_with_its_sequence() -> Result<(), Box<dyn Error>> {
    let frames = history()?;
    assert_eq!(frames.len(), 9);
    for (index, frame) in frames.iter().take(8).enumerate() {
        let seq = match frame {
            ServerFrame::Event { seq, .. } | ServerFrame::PermissionRequest { seq } => Some(*seq),
            ServerFrame::Error { seq, .. } => *seq,
            _ => None,
        };
        assert_eq!(seq, Some(Seq::from(index as u64 + 1)), "line {}", index + 1);
    }
    Ok(())
}

#[test]
fn proto_u_027_retired_types_read_back_as_markers_without_their_fields()
-> Result<(), Box<dyn Error>> {
    let frames = history()?;
    let event = |index: usize| match &frames[index] {
        ServerFrame::Event { event, .. } => Ok(event.clone()),
        _ => Err(format!("line {} is not an event", index + 1)),
    };
    assert_eq!(
        serde_json::to_value(&frames[0])?,
        json!({"type":"permission.request","seq":1})
    );
    assert_eq!(event(1)?, Event::PermissionResolved {});
    assert_eq!(event(2)?, Event::PermissionExpired {});
    assert_eq!(event(3)?, Event::RunParked {});
    assert_eq!(
        event(4)?,
        Event::SandboxState {
            state: SandboxState::Parked
        }
    );
    let Event::ToolResult { content, .. } = event(5)? else {
        return Err("line 6 is not a tool result".into());
    };
    assert_eq!(
        content, "denied",
        "the old denial field is ignored, the result kept"
    );
    let Event::RunFailed { code, .. } = event(6)? else {
        return Err("line 7 is not a failed run".into());
    };
    assert_eq!(code, ErrorCode::PermissionBinding);
    assert!(matches!(
        &frames[7],
        ServerFrame::Error {
            code: ErrorCode::PermissionBinding,
            ..
        }
    ));
    let ServerFrame::SessionReady { sandbox, .. } = &frames[8] else {
        return Err("line 9 is not session.ready".into());
    };
    assert_eq!(sandbox.state, SandboxState::Parked);
    for (index, frame) in frames.iter().enumerate().skip(1).take(3) {
        let value: Value = serde_json::to_value(frame)?;
        let fields = value["event"].as_object().ok_or("event object")?.len();
        assert_eq!(fields, 1, "line {}: only the type is kept", index + 1);
    }
    Ok(())
}

#[test]
fn proto_u_027_the_approval_client_frames_are_gone() -> Result<(), Box<dyn Error>> {
    for frame in [
        json!({"type":"permission.decision","request_id":"q","decision":"allow_once"}),
        json!({"type":"permission.forward","request_id":"q","decision":"deny"}),
        json!({"type":"session.park","request_id":"q"}),
    ] {
        let error = ClientFrame::parse(&serde_json::to_vec(&frame)?)
            .err()
            .ok_or("parsed")?;
        assert_eq!(error.code, ErrorCode::UnknownFrame, "{frame}");
    }
    Ok(())
}
