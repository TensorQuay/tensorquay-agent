//! CORE-U-090: pictures the agent read from its workspace in the model's request. `read_file` names a picture as one
//! (TOOL-U-143); on a route that reads pictures the latest three go as images on their own results, and an older one
//! gets the fixed line; on a route without vision every one gets the line that says the model doesn't read pictures.
//! The owner's case (13 Nov 2026): the agent converted a photo to JPEG in its workspace and still couldn't see it.
mod support;
use std::{error::Error, fs, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{CompletionRequest, ContentPart, Role};
use tqa_tools::{ToolRegistry, builtin::ReadFile};

const NOT_SHOWN: &str =
    "[This picture is no longer shown. Read it again with read_file to see it.]";
const NO_VISION: &str = "[This model doesn't read pictures, so the picture isn't shown; describe what you need from it in words.]";

fn picture(number: u8) -> Vec<u8> {
    [
        b"\x89PNG\r\n\x1a\n".as_slice(),
        &[0, 0, 0, 13],
        b"IHDR",
        &[number],
    ]
    .concat()
}

fn harness(vision: bool) -> Result<Harness, Box<dyn Error>> {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(ReadFile::new()))?;
    let read = |id: &str, path: &str| (id.to_owned(), format!(r#"{{"path":"{path}"}}"#));
    let steps: Vec<(String, String)> = vec![
        read("p1", "uploads/IMG_1.png"),
        read("p2", "site/img/2.png"),
        read("t1", "notes.txt"),
        read("p3", "./3.png"),
        read("p4", "uploads/IMG_4.png"),
    ];
    let mut script: Vec<_> = steps
        .iter()
        .map(|(id, args)| calls(&[(id.as_str(), "read_file", args.as_str())]))
        .collect();
    script.push(answer());
    let mut h = Harness::new(script, tools)?;
    h.deps.route.vision = vision;
    let root = h.directory.path().to_owned();
    fs::create_dir_all(root.join("uploads"))?;
    fs::create_dir_all(root.join("site/img"))?;
    fs::write(root.join("uploads/IMG_1.png"), picture(1))?;
    fs::write(root.join("site/img/2.png"), picture(2))?;
    fs::write(root.join("notes.txt"), "plain words\n")?;
    fs::write(root.join("3.png"), picture(3))?;
    fs::write(root.join("uploads/IMG_4.png"), picture(4))?;
    Ok(h)
}

/// Each `read_file` result of a request: (call id, its images, its text).
fn results(request: &CompletionRequest) -> Vec<(String, Vec<Vec<u8>>, String)> {
    request
        .messages
        .iter()
        .filter(|message| message.role == Role::Tool)
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

async fn last_request(h: Harness) -> Result<CompletionRequest, Box<dyn Error>> {
    let provider = h.provider.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    Ok(provider.requests().last().cloned().ok_or("no request")?)
}

// TEST: CORE-U-090
#[tokio::test]
async fn core_u_090_the_latest_three_pictures_read_go_on_their_results() -> TestResult {
    let sent = results(&last_request(harness(true)?).await?);
    let ids: Vec<&str> = sent.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(ids, ["p1", "p2", "t1", "p3", "p4"]);
    // The oldest picture isn't sent and says so on its own line; the text file is untouched.
    assert!(sent[0].1.is_empty());
    assert!(
        sent[0].2.ends_with(&format!("\n{NOT_SHOWN}")),
        "{}",
        sent[0].2
    );
    assert!(
        sent[2].1.is_empty() && sent[2].2.contains("plain words"),
        "{}",
        sent[2].2
    );
    // The latest three, each its own bytes, whatever form its path took (an upload, a folder, ./...).
    for (index, number) in [(1, 2), (3, 3), (4, 4)] {
        assert_eq!(sent[index].1, vec![picture(number)], "picture {number}");
        assert!(!sent[index].2.contains(NOT_SHOWN), "{}", sent[index].2);
    }
    Ok(())
}

// TEST: CORE-U-090
#[tokio::test]
async fn core_u_090_a_model_without_vision_is_told_and_sent_none() -> TestResult {
    let sent = results(&last_request(harness(false)?).await?);
    for (id, images, text) in sent.iter().filter(|(id, _, _)| id.starts_with('p')) {
        assert!(images.is_empty(), "{id}");
        assert!(text.ends_with(&format!("\n{NO_VISION}")), "{id}: {text}");
    }
    Ok(())
}
