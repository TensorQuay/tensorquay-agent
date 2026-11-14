//! CORE-U-081 (ADR-0039 section 3): a run's `computer` screenshots in the model's request. The latest three go as
//! images on their own tool results, on a route with images; an older one, or one whose file is gone, goes as the
//! fixed line. Only the latest screenshot carries its marks, and only while no later observe replaced its snapshot;
//! the others go as their clean copies, so a mark's number the model reads always means the current snapshot. The
//! bytes are kept in the runtime's state (never the workspace), never journaled, and removed when the run ends.
mod support;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner, Screenshots};
use tqa_llm::{CompletionRequest, ContentPart, Role};
use tqa_tools::{
    Tool, ToolContext, ToolError, ToolImage, ToolName, ToolOutput, ToolRegistry, ToolSpec,
};

const NOT_SHOWN: &str =
    "[This screenshot is no longer shown. Take another to see the screen as it is now.]";

/// The text ends with the fixed line, and a newline parts it from the result's own text (a refusal and the line ran
/// together in a session before 17.0).
fn parted(text: &str) -> bool {
    text.strip_suffix(NOT_SHOWN)
        .is_some_and(|before| before.ends_with('\n'))
}

/// A `computer` tool whose every screenshot is a distinct marked PNG, `PNG1`, `PNG2`, ..., with its clean copy,
/// `CLEAN1`, `CLEAN2`, ...
struct Camera {
    spec: ToolSpec,
    taken: AtomicU8,
}

impl Camera {
    fn new() -> Result<Self, ToolError> {
        Ok(Self {
            spec: ToolSpec {
                name: ToolName::new("computer")?,
                description: "Test computer".into(),
                parameters: json!({"type": "object", "required": ["action"],
                                   "properties": {"action": {"type": "string"}}, "additionalProperties": false}),
                parallel_safe: false,
                changes_workspace: false,
            },
            taken: AtomicU8::new(0),
        })
    }
}

#[async_trait]
impl Tool for Camera {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn call(&self, args: Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        if args["action"] != "screenshot" {
            return Ok(ToolOutput::text("Pressed Tab; no visible change."));
        }
        let number = self.taken.fetch_add(1, Ordering::SeqCst) + 1;
        let mut output = ToolOutput::text(format!("Screenshot {number}."));
        output.image = Some(ToolImage {
            png: png(number),
            width: 2,
            height: 2,
            marked: true,
            plain: Some(clean(number)),
        });
        Ok(output)
    }
}

fn png(number: u8) -> Vec<u8> {
    format!("\u{89}PNG{number}").into_bytes()
}

fn clean(number: u8) -> Vec<u8> {
    format!("\u{89}CLEAN{number}").into_bytes()
}

/// Four screenshots and a key press, one call a step, then (when `observed`) an observe, then the answer.
fn script(observed: bool) -> Vec<tqa_testkit::ProviderReply> {
    let shot = r#"{"action":"screenshot"}"#;
    let mut replies = vec![
        calls(&[("s1", "computer", shot)]),
        calls(&[("s2", "computer", shot)]),
        calls(&[("k1", "computer", r#"{"action":"key"}"#)]),
        calls(&[("s3", "computer", shot)]),
        calls(&[("s4", "computer", shot)]),
    ];
    if observed {
        replies.push(calls(&[("o1", "computer", r#"{"action":"observe"}"#)]));
    }
    replies.push(answer());
    replies
}

fn harness(vision: bool) -> Result<(Harness, tempfile::TempDir), Box<dyn Error>> {
    harness_with(vision, false)
}

fn harness_with(
    vision: bool,
    observed: bool,
) -> Result<(Harness, tempfile::TempDir), Box<dyn Error>> {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(Camera::new()?))?;
    let mut h = Harness::new(script(observed), tools)?;
    h.deps.route.vision = vision;
    let state = tempfile::tempdir()?;
    h.deps.screenshots = Some(Screenshots::new(&state.path().join("screenshots")));
    Ok((h, state))
}

/// Each screenshot result of a request: (call id, its images, its text).
fn shots(request: &CompletionRequest) -> Vec<(String, Vec<Vec<u8>>, String)> {
    request
        .messages
        .iter()
        .filter(|message| message.role == Role::Tool)
        .filter(|message| {
            message
                .tool_call_id
                .as_deref()
                .is_some_and(|id| id.starts_with('s'))
        })
        .map(|message| {
            let images = message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Image { data, .. } => Some(data.clone()),
                    _ => None,
                })
                .collect();
            let text = message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (
                message.tool_call_id.clone().unwrap_or_default(),
                images,
                text,
            )
        })
        .collect()
}

async fn run(h: Harness) -> Result<RunOutcome, Box<dyn Error>> {
    Ok(Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

// TEST: CORE-U-081
#[tokio::test]
async fn core_u_081_the_latest_three_screenshots_go_on_their_results() -> TestResult {
    let (h, _state) = harness(true)?;
    let provider = h.provider.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let requests = provider.requests();
    let last = requests.last().ok_or("no request")?;
    let sent = shots(last);
    let ids: Vec<&str> = sent.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(ids, ["s1", "s2", "s3", "s4"]);
    assert!(sent[0].1.is_empty(), "the oldest isn't sent");
    assert!(parted(&sent[0].2), "{}", sent[0].2);
    for (index, (_, images, text)) in sent.iter().enumerate().skip(1) {
        let number = u8::try_from(index + 1)?;
        // Only the latest carries marks; an older picture's mark 7 could name another element than the current e7.
        let expected = if number == 4 {
            png(number)
        } else {
            clean(number)
        };
        assert_eq!(
            images,
            &vec![expected],
            "screenshot {number}'s own bytes on its own result"
        );
        assert_eq!(
            text,
            &format!("Screenshot {number}."),
            "no fixed line on a sent one"
        );
    }
    // The request before the second screenshot carried the first.
    assert_eq!(shots(&requests[1])[0].1, vec![png(1)]);
    Ok(())
}

// TEST: CORE-U-081
#[tokio::test]
async fn core_u_081_a_later_observe_leaves_every_screenshot_unmarked() -> TestResult {
    let (h, _state) = harness_with(true, true)?;
    let provider = h.provider.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let last = provider.requests().pop().ok_or("no request")?;
    let sent: Vec<Vec<Vec<u8>>> = shots(&last)
        .into_iter()
        .map(|(_, images, _)| images)
        .collect();
    assert_eq!(
        sent,
        vec![vec![], vec![clean(2)], vec![clean(3)], vec![clean(4)]],
        "the latest screenshot's snapshot isn't the current one: no marks anywhere"
    );
    Ok(())
}

// TEST: CORE-U-081
#[tokio::test]
async fn core_u_081_screenshots_are_never_journaled_and_leave_when_the_next_run_starts()
-> TestResult {
    let (h, state) = harness(true)?;
    let workspace = h.directory.path().to_owned();
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let journal = serde_json::to_string(&sink.events())?;
    assert!(
        !journal.contains("PNG") && !journal.contains("CLEAN"),
        "no screenshot bytes in the journal"
    );
    let kept = std::fs::read_dir(state.path().join("screenshots"))?.count();
    assert_eq!(
        kept, 1,
        "kept after the run: nothing is awaited after its last event"
    );
    // The next run removes them before it starts.
    let mut next = Harness::new(vec![answer()], ToolRegistry::new())?;
    next.deps.screenshots = Some(Screenshots::new(&state.path().join("screenshots")));
    assert_eq!(run(next).await?, RunOutcome::Completed);
    let left = std::fs::read_dir(state.path().join("screenshots"))?.count();
    assert_eq!(
        left, 0,
        "an earlier run's screenshots go when the next run starts"
    );
    let in_workspace = walk(&workspace)
        .into_iter()
        .any(|path| path.extension().is_some_and(|extension| extension == "png"));
    assert!(!in_workspace, "never kept in the workspace");
    Ok(())
}

// TEST: CORE-U-081
#[tokio::test]
async fn core_u_081_a_route_without_images_or_a_lost_file_gets_the_fixed_line() -> TestResult {
    let (h, _state) = harness(false)?;
    let provider = h.provider.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let last = provider.requests().pop().ok_or("no request")?;
    for (id, images, text) in shots(&last) {
        assert!(
            images.is_empty(),
            "{id}: no image on a route without images"
        );
        assert!(parted(&text), "{id}: {text}");
    }
    // Without a store (a lost sandbox's files), every screenshot is the fixed line too.
    let (mut h, _state) = harness(true)?;
    h.deps.screenshots = None;
    let provider = h.provider.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let last = provider.requests().pop().ok_or("no request")?;
    assert!(
        shots(&last)
            .iter()
            .all(|(_, images, text)| images.is_empty() && parted(text))
    );
    Ok(())
}

/// A write that fails leaves no `.partial` file behind to count against the state quota: here the clean copy's place
/// is taken by a directory, so its partial file is written and the rename onto it fails.
#[tokio::test]
async fn core_u_081_a_failed_write_leaves_no_partial_file() -> TestResult {
    let state = tempfile::tempdir()?;
    let store = Screenshots::new(&state.path().join("screenshots"));
    let (run, call) = ("run_1".parse()?, "call_1".parse()?);
    let dir = state.path().join("screenshots/run_1");
    std::fs::create_dir_all(dir.join("000000-call_1.plain.png/taken"))?;

    let kept = store.keep(&run, &call, b"PNG1", Some(b"CLEAN1")).await;

    assert!(kept.is_err(), "the clean copy's rename should fail");
    let partials: Vec<_> = walk(&dir)
        .into_iter()
        .filter(|path| path.to_string_lossy().ends_with(".partial"))
        .collect();
    assert!(partials.is_empty(), "left behind: {partials:?}");
    assert_eq!(store.read(&run, &call, false).await, None);
    Ok(())
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

// TEST: CORE-U-084
// TEST: CORE-U-081
/// A request's screenshots are images like any other: each step call's usage records them in `estimated_images`
/// (decided on 7 Oct 2026), so a desktop run's density takes them out of the provider's count and isn't
/// read higher than its text's. Only the screenshots sent count: the latest three on a route with images, none on one
/// without. The script's six requests have 0, 1, 2, 2 (a key press between), 3 and 3 (four taken, three sent).
#[tokio::test]
async fn core_u_084_a_desktop_runs_usage_counts_its_screenshots_as_images() -> TestResult {
    let image = u64::from(tqa_types::IMAGE_TOKENS);
    for (vision, sent) in [(true, [0_u64, 1, 2, 2, 3, 3]), (false, [0; 6])] {
        let (h, _state) = harness(vision)?;
        let sink = h.sink.clone();
        assert_eq!(run(h).await?, RunOutcome::Completed);
        let recorded: Vec<Option<u64>> = sink
            .events()
            .into_iter()
            .filter_map(|event| match event {
                tqa_types::Event::Usage {
                    estimated_images, ..
                } => Some(estimated_images),
                _ => None,
            })
            .collect();
        let expected: Vec<Option<u64>> = sent.iter().map(|shots| Some(shots * image)).collect();
        assert_eq!(recorded, expected, "vision {vision}");
    }
    Ok(())
}
