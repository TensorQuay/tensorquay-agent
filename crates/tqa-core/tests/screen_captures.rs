//! CORE-U-083 (ADR-0039 section 9 and "The record"): a `computer` call's captures for the person's record. Each is
//! uploaded to the screenshot store and journaled as `screen.capture` only once the store holds it: a before capture
//! ahead of its call's `tool.start`, an after capture or the model's screenshot ahead of its `tool.result`. The result's
//! `evidence` says which were kept; one refused or unanswered is dropped without an event, its phase false, and the
//! model is told "(evidence not kept)" for an action, "(not kept for the record)" for its own screenshot. Without a
//! store nothing is uploaded and nothing is said.
mod support;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{AFTER_WAIT, BEFORE_WAIT, RunOutcome, Runner, ScreenStore, Unstored, screenshot_id};
use tqa_tools::{
    Tool, ToolContext, ToolError, ToolEvidence, ToolImage, ToolName, ToolOutput, ToolRegistry,
    ToolSpec,
};
use tqa_types::{Event, ScreenEvidence, StoreRefusal};

/// A desktop whose click takes evidence (`BEFORE`, `AFTER`), whose screenshot is a marked `SHOT`, and whose observe
/// takes nothing. It counts its pre-start steps.
struct Desk {
    spec: ToolSpec,
    befores: AtomicU8,
}

impl Desk {
    fn new() -> Result<Arc<Self>, ToolError> {
        Self::named("computer")
    }

    fn named(name: &str) -> Result<Arc<Self>, ToolError> {
        Ok(Arc::new(Self {
            spec: ToolSpec {
                name: ToolName::new(name)?,
                description: "Test computer".into(),
                parameters: json!({"type": "object", "required": ["action"],
                                   "properties": {"action": {"type": "string"}}, "additionalProperties": false}),
                parallel_safe: false,
                changes_workspace: false,
            },
            befores: AtomicU8::new(0),
        }))
    }
}

fn image(bytes: &str, marked: bool) -> ToolImage {
    ToolImage {
        png: format!("\u{89}PNG{bytes}").into_bytes(),
        width: 1280,
        height: 800,
        marked,
        plain: None,
    }
}

#[async_trait]
impl Tool for Desk {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn before(&self, args: &Value, _: &ToolContext) -> Option<ToolImage> {
        self.befores.fetch_add(1, Ordering::SeqCst);
        (args["action"] == "click").then(|| image("BEFORE", false))
    }

    async fn call(&self, args: Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        Ok(match args["action"].as_str() {
            Some("click") => {
                let mut output = ToolOutput::text("Clicked e2; no visible change.");
                output.evidence = Some(ToolEvidence {
                    after: Some(image("AFTER", false)),
                });
                output
            }
            Some("screenshot") => {
                let mut output = ToolOutput::text("Screenshot 1280×800.");
                output.image = Some(image("SHOT", true));
                output
            }
            _ => ToolOutput::text("snapshot 1"),
        })
    }
}

/// A store answering from a script (then `Ok`), recording each upload's id, size and wait.
#[derive(Default)]
struct Store {
    answers: Mutex<VecDeque<Result<(), Unstored>>>,
    uploads: Mutex<Vec<(String, usize, Duration)>>,
}

#[async_trait]
impl ScreenStore for Store {
    async fn store(&self, id: &str, png: &[u8], wait: Duration) -> Result<(), Unstored> {
        if let Ok(mut uploads) = self.uploads.lock() {
            uploads.push((id.to_owned(), png.len(), wait));
        }
        self.answers
            .lock()
            .ok()
            .and_then(|mut answers| answers.pop_front())
            .unwrap_or(Ok(()))
    }
}

/// A click, a screenshot and an observe, one a step, then another tool's image (no screenshot of the desktop), then
/// the answer.
fn script() -> Vec<tqa_testkit::ProviderReply> {
    vec![
        calls(&[("c1", "computer", r#"{"action":"click"}"#)]),
        calls(&[("s1", "computer", r#"{"action":"screenshot"}"#)]),
        calls(&[("o1", "computer", r#"{"action":"observe"}"#)]),
        calls(&[("p1", "picture", r#"{"action":"screenshot"}"#)]),
        answer(),
    ]
}

async fn run(store: Option<Arc<Store>>, desk: Arc<Desk>) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut tools = ToolRegistry::new();
    tools.register(desk)?;
    tools.register(Desk::named("picture")?)?;
    let mut h = Harness::new(script(), tools)?;
    h.deps.screen_store = store.map(|store| store as Arc<dyn ScreenStore>);
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    Ok(sink.events())
}

/// The log, one line per event that matters here: `capture <call> <phase>`, `start <call>`, `result <call>`.
fn order(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ScreenCapture { call_id, phase, .. } => {
                Some(format!("capture {call_id} {phase:?}"))
            }
            Event::ToolStart { call_id } => Some(format!("start {call_id}")),
            Event::ToolResult { call_id, .. } => Some(format!("result {call_id}")),
            _ => None,
        })
        .collect()
}

/// Each result's evidence and content, by call.
fn results(events: &[Event]) -> Vec<(String, Option<ScreenEvidence>, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ToolResult {
                call_id,
                evidence,
                content,
                ..
            } => Some((call_id.to_string(), *evidence, content.clone())),
            _ => None,
        })
        .collect()
}

/// What each `screen.capture` names: (id, sha256, bytes, sides, sent to the model, marked).
type Named = (String, String, u64, (u32, u32), bool, bool);

fn captures(events: &[Event]) -> Vec<Named> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ScreenCapture {
                id,
                sha256,
                bytes,
                width,
                height,
                sent_to_model,
                marked,
                ..
            } => Some((
                id.clone(),
                sha256.clone(),
                *bytes,
                (*width, *height),
                *sent_to_model,
                *marked,
            )),
            _ => None,
        })
        .collect()
}

/// What a capture of the fake's `name` image should name once stored.
fn stored(name: &str, marked: bool) -> Named {
    let png = image(name, marked).png;
    let id = screenshot_id(&png);
    (
        id.clone(),
        id,
        u64::try_from(png.len()).unwrap_or(0),
        (1280, 800),
        marked,
        marked,
    )
}

// TEST: CORE-U-083
#[tokio::test]
async fn core_u_083_captures_are_journaled_in_log_order_once_stored() -> Result<(), Box<dyn Error>>
{
    let store = Arc::new(Store::default());
    let events = run(Some(store.clone()), Desk::new()?).await?;
    assert_eq!(
        order(&events),
        [
            "capture c1 Before",
            "start c1",
            "capture c1 After",
            "result c1",
            "start s1",
            "capture s1 Screenshot",
            "result s1",
            "start o1",
            "result o1",
            "start p1",
            "result p1",
        ]
    );
    let kept_both = ScreenEvidence {
        before: Some(true),
        after: true,
    };
    let kept_shot = ScreenEvidence {
        before: None,
        after: true,
    };
    assert_eq!(
        results(&events),
        [
            (
                "c1".into(),
                Some(kept_both),
                "Clicked e2; no visible change.".into()
            ),
            ("s1".into(), Some(kept_shot), "Screenshot 1280×800.".into()),
            ("o1".into(), None, "snapshot 1".into()),
            ("p1".into(), None, "Screenshot 1280×800.".into()),
        ]
    );
    // Each event names exactly what was stored: id and sha256 the PNG's SHA-256, its size and sides, and only the
    // model's own screenshot sent and marked.
    assert_eq!(
        captures(&events),
        [
            stored("BEFORE", false),
            stored("AFTER", false),
            stored("SHOT", true)
        ]
    );
    let waits: Vec<Duration> = store
        .uploads
        .lock()
        .map_err(|_| "poisoned")?
        .iter()
        .map(|(_, _, wait)| *wait)
        .collect();
    assert_eq!(waits, [BEFORE_WAIT, AFTER_WAIT, AFTER_WAIT]);
    Ok(())
}

// TEST: CORE-U-083
#[tokio::test]
async fn core_u_083_a_capture_not_kept_is_dropped_and_its_phase_reads_false()
-> Result<(), Box<dyn Error>> {
    let store = Arc::new(Store::default());
    if let Ok(mut answers) = store.answers.lock() {
        answers.extend([
            Err(Unstored::Refused(StoreRefusal::QuotaFull)),
            Err(Unstored::NoAnswer),
            Err(Unstored::Refused(StoreRefusal::TooLarge)),
        ]);
    }
    let events = run(Some(store), Desk::new()?).await?;
    assert!(
        !order(&events)
            .iter()
            .any(|line| line.starts_with("capture")),
        "no event names a screenshot the store doesn't hold"
    );
    assert_eq!(
        results(&events),
        [
            (
                "c1".into(),
                Some(ScreenEvidence {
                    before: Some(false),
                    after: false
                }),
                "Clicked e2; no visible change.\n(evidence not kept)".into()
            ),
            (
                "s1".into(),
                Some(ScreenEvidence {
                    before: None,
                    after: false
                }),
                "Screenshot 1280×800.\n(not kept for the record)".into()
            ),
            ("o1".into(), None, "snapshot 1".into()),
            ("p1".into(), None, "Screenshot 1280×800.".into()),
        ]
    );
    Ok(())
}

// TEST: CORE-U-083
#[tokio::test]
async fn core_u_083_without_a_store_nothing_is_uploaded_or_said() -> Result<(), Box<dyn Error>> {
    let desk = Desk::new()?;
    let events = run(None, desk.clone()).await?;
    assert!(
        !order(&events)
            .iter()
            .any(|line| line.starts_with("capture"))
    );
    assert!(
        results(&events)
            .iter()
            .all(|(_, evidence, content)| evidence.is_none()
                && !content.contains("evidence not kept")),
        "{:?}",
        results(&events)
    );
    assert_eq!(
        desk.befores.load(Ordering::SeqCst),
        0,
        "no screen taken for nothing"
    );
    Ok(())
}
