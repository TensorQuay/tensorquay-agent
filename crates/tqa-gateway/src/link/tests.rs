use super::{FrameSink, LinkRegistry, Received, StoreBounds, receive};
use crate::{GatewayError, wire};
use async_trait::async_trait;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::ReadinessSource;
use tqa_types::{
    ClientFrame, ConversationId, ErrorCode, SandboxBackend, SandboxState, SandboxSummary,
    SeqChecker, ServerFrame,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BOUNDS: StoreBounds = StoreBounds::new(std::time::Duration::from_secs(10));

#[tokio::test]
async fn gw_i_006_runtime_queues_fail_closed_at_capacity_and_after_shutdown() -> TestResult {
    let cancel = CancellationToken::new();
    let registry = LinkRegistry::new(1, 1, cancel.clone());
    let id: ConversationId = "bounded".parse()?;
    let mut registration = registry.register(id.clone(), "instance".parse()?)?;
    let permit = registry.reserve(&id)?;
    assert!(matches!(registry.reserve(&id), Err(GatewayError::Busy)));
    drop(permit);
    registration.actions.close();
    assert!(matches!(
        registry.reserve(&id),
        Err(GatewayError::Conflict("sandbox_not_ready"))
    ));
    drop(registration);
    let _replacement = registry.register("another".parse()?, "next".parse()?)?;
    registry.remove(&id)?;
    cancel.cancel();
    assert!(matches!(
        registry.register(id, "last".parse()?),
        Err(GatewayError::Cancelled)
    ));
    Ok(())
}

#[derive(Default)]
struct Store {
    durable: Mutex<Vec<ServerFrame>>,
    controls: Mutex<Vec<ServerFrame>>,
}

#[async_trait]
impl FrameSink for Store {
    async fn append(&self, _: &ConversationId, frame: ServerFrame) -> Result<(), GatewayError> {
        let mut frames = self.durable.lock().map_err(|_| GatewayError::Io)?;
        let last = frames.last().and_then(wire::sequence).unwrap_or_default();
        SeqChecker::new(last)
            .accept(wire::sequence(&frame).ok_or(GatewayError::BadRequest("unsequenced"))?)?;
        frames.push(frame);
        Ok(())
    }
    fn control(&self, _: &ConversationId, frame: ServerFrame) -> Result<(), GatewayError> {
        self.controls
            .lock()
            .map_err(|_| GatewayError::Io)?
            .push(frame);
        Ok(())
    }
}

#[tokio::test]
async fn proto_u_004_link_rejects_a_gap_then_accepts_correct_sequence_on_redial() -> TestResult {
    let store = Store::default();
    let id = "conversation".parse()?;
    for (seq, accepted) in [(1_u64, true), (3, false), (2, true)] {
        let frame = ServerFrame::Error {
            seq: Some(seq.into()),
            code: ErrorCode::Io,
            message: String::new(),
            retryable: false,
            target: None,
        };
        let result = receive(&store, &id, &serde_json::to_vec(&frame)?, BOUNDS).await;
        if accepted {
            result?;
        } else {
            assert_eq!(result, Err(GatewayError::Protocol(ErrorCode::SeqGap)));
        }
    }
    assert_eq!(store.durable.lock().map_err(|_| "lock")?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn proto_u_003_control_frames_are_ephemeral_and_bound_to_the_authenticated_conversation()
-> TestResult {
    let store = Store::default();
    let id: ConversationId = "conversation".parse()?;
    let ready = |conversation_id| ServerFrame::SessionReady {
        conversation_id,
        resumed: false,
        sandbox: SandboxSummary::new(SandboxBackend::Docker, SandboxState::Ready),
        queue: None,
    };
    receive(
        &store,
        &id,
        &serde_json::to_vec(&ready(id.clone()))?,
        BOUNDS,
    )
    .await?;
    assert!(
        receive(
            &store,
            &id,
            &serde_json::to_vec(&ready("foreign".parse()?))?,
            BOUNDS
        )
        .await
        .is_err()
    );
    let error = ServerFrame::Error {
        seq: None,
        code: ErrorCode::Internal,
        message: "failed".into(),
        retryable: true,
        target: None,
    };
    receive(&store, &id, &serde_json::to_vec(&error)?, BOUNDS).await?;
    // A busy refusal is handed to the link, which puts a held turn back or forwards it (GW-I-129).
    let busy = ServerFrame::Error {
        seq: None,
        code: ErrorCode::Busy,
        message: "busy".into(),
        retryable: true,
        target: None,
    };
    assert_eq!(
        receive(&store, &id, &serde_json::to_vec(&busy)?, BOUNDS).await?,
        super::Received::Busy(Box::new(busy))
    );
    assert_eq!(
        receive(&store, &id, br#"{"type":"future"}"#, BOUNDS).await,
        Err(GatewayError::Protocol(ErrorCode::UnknownFrame))
    );
    assert!(
        receive(
            &store,
            &id,
            &vec![b' '; tqa_types::MAX_FRAME_BYTES + 1],
            BOUNDS
        )
        .await
        .is_err()
    );
    assert!(store.durable.lock().map_err(|_| "lock")?.is_empty());
    assert_eq!(store.controls.lock().map_err(|_| "lock")?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn gw_i_040_supervisor_heartbeats_never_reach_the_store_or_client_fanout() -> TestResult {
    let store = Store::default();
    let id = "conversation".parse()?;
    let run_id = "run-live".parse()?;
    let heartbeat = ServerFrame::Heartbeat { run_id };
    let received = receive(&store, &id, &serde_json::to_vec(&heartbeat)?, BOUNDS).await?;
    assert_eq!(received, Received::Heartbeat("run-live".parse()?));
    assert!(store.durable.lock().map_err(|_| "lock")?.is_empty());
    assert!(store.controls.lock().map_err(|_| "lock")?.is_empty());
    let registry = LinkRegistry::new(1, 1, CancellationToken::new());
    let first = registry.register(id.clone(), "instance".parse()?)?;
    let now = tokio::time::Instant::now();
    registry.saw_heartbeat(&id, first.generation, "run-live".parse()?, now)?;
    assert_eq!(
        registry
            .heartbeat(&id)?
            .ok_or("missing heartbeat")?
            .received_at,
        now
    );
    let _second = registry.register(id.clone(), "next-instance".parse()?)?;
    registry.saw_heartbeat(&id, first.generation, "stale-run".parse()?, now)?;
    assert!(registry.heartbeat(&id)?.is_none());
    Ok(())
}

#[tokio::test]
async fn gw_i_006_replacement_cancels_the_old_link_without_unregistering_the_new_one() -> TestResult
{
    let cancel = CancellationToken::new();
    let registry = LinkRegistry::new(1, 1, cancel.clone());
    let id = "conversation".parse()?;
    assert!(!registry.is_ready(&id).await);
    assert!(registry.reserve(&id).is_err());
    let first = registry.register(id.clone(), "sandbox-one".parse()?)?;
    assert!(registry.is_ready(&id).await);
    assert!(
        registry
            .register("other".parse()?, "sandbox-other".parse()?)
            .is_err()
    );
    let mut second = registry.register(id.clone(), "sandbox-two".parse()?)?;
    assert!(first.cancel.is_cancelled());
    registry.unregister(&id, first.generation)?;
    assert!(registry.is_ready(&id).await);
    let frame = ClientFrame::UserMessage {
        lead_brief: None,
        team_wake: None,
        text: "hello".into(),
        attachments: vec![],
        host_keys: Vec::new(),
    };
    let (sent, received) = tokio::sync::oneshot::channel();
    registry.reserve(&id)?.send(super::registry::Delivery {
        frame: frame.clone(),
        sent,
    });
    let delivered = second.actions.recv().await.ok_or("missing action")?;
    assert_eq!(delivered.frame, frame);
    let _ = delivered.sent.send(());
    received.await?;
    registry.remove(&id)?;
    assert!(second.cancel.is_cancelled());
    assert!(!registry.is_ready(&id).await);
    let _third = registry.register(id.clone(), "sandbox-three".parse()?)?;
    cancel.cancel();
    assert!(!registry.is_ready(&id).await);
    Ok(())
}

#[tokio::test]
async fn sec_003_a_decision_shaped_frame_from_the_runtime_is_never_a_decision() -> TestResult {
    let store = Store::default();
    let id = "conversation".parse()?;
    let forged = serde_json::json!({
        "type": "permission.decision",
        "request_id": "req-1",
        "action_hash": "0".repeat(64),
        "decision": "allow_once"
    });
    assert_eq!(
        receive(&store, &id, &serde_json::to_vec(&forged)?, BOUNDS).await,
        Err(GatewayError::Protocol(ErrorCode::UnknownFrame))
    );
    assert!(store.durable.lock().map_err(|_| "lock")?.is_empty());
    assert!(store.controls.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}
