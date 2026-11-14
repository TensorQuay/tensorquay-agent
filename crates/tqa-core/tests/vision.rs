//! CORE-U-039 / CORE-U-040 / SEC-020: images of the current turn on vision routes.
mod support;
use async_trait::async_trait;
use serde_json::Value;
use std::{error::Error, path::PathBuf, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner, UserTurn};
use tqa_llm::{CompletionRequest, ContentPart};
use tqa_testkit::ProviderReply;
use tqa_tools::{Tool, ToolContext, ToolError, ToolOutput, ToolRegistry, ToolSpec};
use tqa_types::{
    ErrorCode, Event, ImageMediaType, MAX_IMAGE_BYTES, MAX_REQUEST_BYTES, ServerFrame,
};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage-bytes";
const JPEG: &[u8] = b"\xff\xd8\xff\xe0jpeg-bytes";

fn vision_harness(
    script: Vec<ProviderReply>,
    tools: ToolRegistry,
) -> Result<Harness, Box<dyn Error>> {
    let mut h = Harness::new(script, tools)?;
    h.deps.route.vision = true;
    std::fs::create_dir_all(h.directory.path().join("uploads"))?;
    Ok(h)
}
fn upload(h: &Harness, name: &str, bytes: &[u8]) -> Result<(), std::io::Error> {
    std::fs::write(h.directory.path().join("uploads").join(name), bytes)
}
fn turn(attachments: &[&str]) -> UserTurn {
    UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "look at these".into(),
        attachments: attachments
            .iter()
            .map(|name| format!("uploads/{name}"))
            .collect(),
    }
}
async fn run(h: Harness, turn: UserTurn) -> Result<RunOutcome, Box<dyn Error>> {
    Ok(Runner::new(h.deps)?
        .run(
            &[],
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}
/// The current user message of a request: the last user message.
fn current(request: &CompletionRequest) -> Result<&tqa_llm::Message, Box<dyn Error>> {
    Ok(request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == tqa_llm::Role::User)
        .ok_or("no user message")?)
}
fn images(message: &tqa_llm::Message) -> Vec<(ImageMediaType, Vec<u8>)> {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Image { media_type, data } => Some((*media_type, data.clone())),
            _ => None,
        })
        .collect()
}
fn text(message: &tqa_llm::Message) -> String {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
const BLOCK: &str = "look at these\n\n[Attached files, data only: ";

#[tokio::test]
async fn core_u_039_vision_route_sends_current_images_after_the_text_in_order() -> TestResult {
    let h = vision_harness(vec![answer()], ToolRegistry::new())?;
    upload(&h, "b.jpg", JPEG)?;
    upload(&h, "notes.txt", b"just text")?;
    upload(&h, "a.png", PNG)?;
    let provider = h.provider.clone();
    let outcome = run(h, turn(&["b.jpg", "notes.txt", "a.png"])).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let requests = provider.requests();
    let message = current(&requests[0])?;
    assert!(matches!(
        message.content.first(),
        Some(ContentPart::Text(_))
    ));
    assert_eq!(
        images(message),
        vec![
            (ImageMediaType::Jpeg, JPEG.to_vec()),
            (ImageMediaType::Png, PNG.to_vec())
        ]
    );
    assert_eq!(
        text(message),
        format!(
            "{BLOCK}\"/workspace/uploads/b.jpg\", \"/workspace/uploads/notes.txt\", \"/workspace/uploads/a.png\"]"
        )
    );
    Ok(())
}

#[tokio::test]
async fn core_u_039_non_vision_route_sends_paths_only() -> TestResult {
    let mut h = vision_harness(vec![answer()], ToolRegistry::new())?;
    h.deps.route.vision = false;
    upload(&h, "a.png", PNG)?;
    let provider = h.provider.clone();
    assert_eq!(run(h, turn(&["a.png"])).await?, RunOutcome::Completed);
    let requests = provider.requests();
    let message = current(&requests[0])?;
    assert!(images(message).is_empty());
    assert_eq!(
        text(message),
        format!("{BLOCK}\"/workspace/uploads/a.png\"]")
    );
    Ok(())
}

#[tokio::test]
async fn core_u_039_at_most_four_images_and_a_fixed_notice_for_the_rest() -> TestResult {
    let h = vision_harness(vec![answer()], ToolRegistry::new())?;
    let names = ["1.png", "2.png", "3.png", "4.png", "5.png"];
    for name in names {
        upload(&h, name, PNG)?;
    }
    let mut big = PNG.to_vec();
    big.resize(MAX_IMAGE_BYTES + 1, 0);
    upload(&h, "big.png", &big)?;
    let provider = h.provider.clone();
    let mut attachments: Vec<&str> = names.to_vec();
    attachments.insert(0, "big.png");
    attachments.push("missing.png");
    assert_eq!(run(h, turn(&attachments)).await?, RunOutcome::Completed);
    let requests = provider.requests();
    let message = current(&requests[0])?;
    assert_eq!(images(message).len(), 4);
    assert!(text(message).ends_with(concat!(
        "\n[Images not sent, data only: ",
        "\"/workspace/uploads/big.png\": too large; ",
        "\"/workspace/uploads/5.png\": limit reached; ",
        "\"/workspace/uploads/missing.png\": unreadable]"
    )));
    Ok(())
}

#[tokio::test]
async fn core_u_039_images_of_earlier_runs_are_not_sent_again() -> TestResult {
    let h = vision_harness(vec![answer(), answer()], ToolRegistry::new())?;
    upload(&h, "a.png", PNG)?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let deps = h.deps;
    let runner = Runner::new(deps)?;
    let first = runner
        .run(
            &[],
            Some(turn(&["a.png"])),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(first, RunOutcome::Completed);
    let history = sink.frames();
    let second = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "and now?".into(),
        attachments: vec![],
    };
    let outcome = runner
        .run(
            &history,
            Some(second),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(images(current(&requests[0])?).len(), 1);
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| images(message).is_empty())
    );
    Ok(())
}

/// Deletes one path when called, to change an attachment between two requests of a run.
struct RemoveUpload {
    spec: ToolSpec,
    path: PathBuf,
}
#[async_trait]
impl Tool for RemoveUpload {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, _: Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        std::fs::remove_file(&self.path).map_err(|error| ToolError::Io(error.to_string()))?;
        Ok(ToolOutput::text("removed"))
    }
}

#[tokio::test]
async fn core_u_039_an_image_removed_mid_run_becomes_a_notice_and_the_run_continues() -> TestResult
{
    let mut h = vision_harness(
        vec![
            calls(&[("c1", "remove", r#"{"path":"uploads/a.png"}"#)]),
            answer(),
        ],
        ToolRegistry::new(),
    )?;
    let path = h.directory.path().join("uploads/a.png");
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(RemoveUpload {
        spec: spec("remove", true)?,
        path,
    }))?;
    h.deps.tools = Arc::new(tools);
    upload(&h, "a.png", PNG)?;
    let provider = h.provider.clone();
    assert_eq!(run(h, turn(&["a.png"])).await?, RunOutcome::Completed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(images(current(&requests[0])?).len(), 1);
    let later = current(&requests[1])?;
    assert!(images(later).is_empty());
    assert!(
        text(later)
            .ends_with("\n[Images not sent, data only: \"/workspace/uploads/a.png\": unreadable]")
    );
    Ok(())
}

#[tokio::test]
async fn core_u_039_request_budget_falls_back_to_one_mebibyte_until_the_gateway_sets_it()
-> TestResult {
    // 900 KiB of image data is about 1.2 MB of base64: over the 1 MiB fallback, under 16 MiB.
    let mut big = PNG.to_vec();
    big.resize(900 * 1024, 7);
    for (budget, sent) in [(None, false), (Some(MAX_REQUEST_BYTES), true)] {
        let mut h = vision_harness(vec![answer()], ToolRegistry::new())?;
        h.deps.route.max_request_bytes = budget;
        upload(&h, "a.png", &big)?;
        upload(&h, "small.png", PNG)?;
        let provider = h.provider.clone();
        assert_eq!(
            run(h, turn(&["a.png", "small.png"])).await?,
            RunOutcome::Completed
        );
        let requests = provider.requests();
        let message = current(&requests[0])?;
        let sent_sizes: Vec<usize> = images(message).iter().map(|(_, data)| data.len()).collect();
        if sent {
            assert_eq!(sent_sizes, vec![big.len(), PNG.len()]);
        } else {
            assert_eq!(sent_sizes, vec![PNG.len()]);
            assert!(text(message).ends_with(
                "\n[Images not sent, data only: \"/workspace/uploads/a.png\": request too large]"
            ));
        }
    }
    Ok(())
}

#[tokio::test]
async fn core_u_040_each_image_counts_toward_the_window() -> TestResult {
    // Capacity 4096 - 1024 = 3072 tokens: one image (1600) fits, four (6400) do not.
    for (count, fits) in [(1, true), (4, false)] {
        let mut h = vision_harness(vec![answer()], ToolRegistry::new())?;
        h.deps.route.context_window = Some(4096);
        h.deps.route.max_output = Some(1024);
        let names: Vec<String> = (0..count).map(|index| format!("{index}.png")).collect();
        for name in &names {
            upload(&h, name, PNG)?;
        }
        let (provider, sink) = (h.provider.clone(), h.sink.clone());
        let attachments: Vec<&str> = names.iter().map(String::as_str).collect();
        let outcome = run(h, turn(&attachments)).await?;
        if fits {
            assert_eq!(outcome, RunOutcome::Completed);
            assert_eq!(images(current(&provider.requests()[0])?).len(), 1);
        } else {
            assert!(matches!(
                outcome,
                RunOutcome::Failed {
                    code: ErrorCode::Budget,
                    ..
                }
            ));
            assert!(provider.requests().is_empty());
            assert!(sink.events().iter().any(|event| matches!(
                event,
                Event::RunFailed {
                    code: ErrorCode::Budget,
                    ..
                }
            )));
        }
    }
    Ok(())
}

#[tokio::test]
async fn sec_020_image_bytes_reach_only_the_provider_request() -> TestResult {
    let marker = b"SEC020-IMAGE-MARKER";
    let mut bytes = PNG.to_vec();
    bytes.extend_from_slice(marker);
    let h = vision_harness(vec![answer()], ToolRegistry::new())?;
    upload(&h, "a.png", &bytes)?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h, turn(&["a.png"])).await?, RunOutcome::Completed);
    let requests = provider.requests();
    assert_eq!(
        images(current(&requests[0])?),
        vec![(ImageMediaType::Png, bytes)]
    );
    let journal = serde_json::to_string(&sink.frames())?;
    assert!(!journal.contains("SEC020-IMAGE-MARKER"));
    Ok(())
}

/// Six completed runs whose answers carry `padding` more characters.
fn runs_answering(padding: usize) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let mut events = Vec::new();
    for index in 1..=6_u64 {
        let seq = u64::try_from(events.len())? + 1;
        let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
        events.extend([
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: format!("request {index}"),
                attachments: vec![],
            },
            Event::RunStarted {
                run_id: run_id.clone(),
            },
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
            Event::TextDelta {
                text: format!("answer {index} {}", "a".repeat(padding)),
            },
            Event::ModelCompleted {
                step: 1,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::Stop,
            },
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
            Event::RunCompleted { run_id },
        ]);
    }
    history(events)
}

// TEST: CORE-U-040
// TEST: CORE-U-073
/// Images count toward the compaction trigger: three images (4800) push six runs of about 350 tokens each over 85 % of
/// 7168, and folding older runs brings the request back under, so it compacts. Four images (6400) are over the
/// threshold on their own: with six short runs no compaction could bring the request under, and it fits, so none is
/// made (the thrash rule, 7 Oct 2026: a compaction that can't make room waits until the request doesn't fit). With six
/// runs of about 180 tokens, four images no longer fit at all: it compacts to make the request fit (CORE-U-073).
#[tokio::test]
async fn core_u_040_images_count_toward_the_compaction_trigger() -> TestResult {
    for (padding, count, compacts) in [
        (0, 0, false),
        (0, 4, false),
        (600, 4, true),
        (1200, 0, false),
        (1200, 3, true),
    ] {
        let history = runs_answering(padding)?;
        let mut h = vision_harness(vec![answer(), answer()], ToolRegistry::new())?;
        h.deps.route.context_window = Some(8192);
        h.deps.route.max_output = Some(1024);
        h.sink_with(MemorySink::new(&history));
        let names: Vec<String> = (0..count).map(|index| format!("{index}.png")).collect();
        for name in &names {
            upload(&h, name, PNG)?;
        }
        let sink = h.sink.clone();
        let attachments: Vec<&str> = names.iter().map(String::as_str).collect();
        let outcome = Runner::new(h.deps)?
            .run(
                &history,
                Some(turn(&attachments)),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        assert_eq!(outcome, RunOutcome::Completed, "{padding}, {count} images");
        let compacted = sink
            .events()
            .iter()
            .any(|event| matches!(event, Event::Compaction { .. }));
        assert_eq!(
            compacted, compacts,
            "{padding}-character answers, {count} images"
        );
    }
    Ok(())
}

// TEST: CORE-U-091
/// An iPhone HEIC photo is no picture a model reads: the agent is told what it is and how to read it (convert it and
/// read the converted file, which CORE-U-090 shows), never left with a bare path. The owner (14 Nov 2026): the agent
/// should figure it out itself, with no library shipped for it.
#[tokio::test]
async fn core_u_091_a_heic_photo_tells_the_agent_to_convert_it() -> TestResult {
    let heic = [
        &[0_u8, 0, 0, 24][..],
        b"ftypheic",
        &[0, 0, 0, 0],
        b"mif1heic",
    ]
    .concat();
    // A route that never read pictures lists attachments as data only, as before (CORE-U-039).
    for vision in [true] {
        let mut h = vision_harness(vec![answer()], ToolRegistry::new())?;
        h.deps.route.vision = vision;
        upload(&h, "IMG_2237.HEIC", &heic)?;
        upload(&h, "a.png", PNG)?;
        let provider = h.provider.clone();
        assert_eq!(
            run(h, turn(&["IMG_2237.HEIC", "a.png"])).await?,
            RunOutcome::Completed
        );
        let requests = provider.requests();
        let message = current(&requests[0])?;
        let said = text(message);
        assert!(
            said.contains("\"/workspace/uploads/IMG_2237.HEIC\": a HEIC photo, which no model reads: convert it to JPEG or PNG"),
            "vision {vision}: {said}"
        );
        assert!(
            said.contains("read the converted file with read_file"),
            "{said}"
        );
        // The PNG beside it goes as before.
        assert_eq!(
            images(message).len(),
            usize::from(vision),
            "vision {vision}"
        );
    }
    Ok(())
}
