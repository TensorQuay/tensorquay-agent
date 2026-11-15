//! CORE-U-092 (PRD item 30): the pages of a PDF the agent read, in the model's request. `read_file` answers a PDF as
//! text by page (TOOL-U-145); on a route that reads pictures the latest three pages go as PNGs on their results and an
//! older result says its pages aren't shown; on a route without vision the text goes as it is, with no line added.
mod support;
use std::{error::Error, fs, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{CompletionRequest, ContentPart, Role};
use tqa_testkit::pdf::pdf;
use tqa_tools::{ToolRegistry, builtin::ReadFile};

const NOT_SHOWN: &str =
    "[These pages are no longer shown as pictures. Read them again with read_file to see them.]";

fn harness(vision: bool) -> Result<Harness, Box<dyn Error>> {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(ReadFile::new()))?;
    let read = |id: &str, args: &str| (id.to_owned(), args.to_owned());
    let steps: Vec<(String, String)> = vec![
        read("d1", r#"{"path":"docs/a.pdf"}"#),
        read("t1", r#"{"path":"notes.txt"}"#),
        read("d2", r#"{"path":"docs/b.pdf","start_line":2,"end_line":3}"#),
    ];
    let mut script: Vec<_> = steps
        .iter()
        .map(|(id, args)| calls(&[(id.as_str(), "read_file", args.as_str())]))
        .collect();
    script.push(answer());
    let mut h = Harness::new(script, tools)?;
    h.deps.route.vision = vision;
    let root = h.directory.path().to_owned();
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("docs/a.pdf"), pdf(&["A one", "A two"]))?;
    fs::write(
        root.join("docs/b.pdf"),
        pdf(&["B one", "B two", "B three", "B four"]),
    )?;
    fs::write(root.join("notes.txt"), "plain words\n")?;
    Ok(h)
}

/// Each `read_file` result of a request: (call id, how many images, its text).
fn results(request: &CompletionRequest) -> Vec<(String, usize, String)> {
    request
        .messages
        .iter()
        .filter(|message| message.role == Role::Tool)
        .map(|message| {
            let images = message
                .content
                .iter()
                .filter(|part| matches!(part, ContentPart::Image { .. }))
                .count();
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

// TEST: CORE-U-092
#[tokio::test]
async fn core_u_092_the_latest_three_pages_go_as_pictures_on_a_vision_route()
-> Result<(), Box<dyn Error>> {
    let results = results(&last_request(harness(true)?).await?);
    // d2, the latest, shows pages 2 and 3 of b.pdf: two pictures; d1 read pages 1 and 2 of a.pdf, and one of them fills
    // the allowance of three; the text file's result is untouched.
    let d2 = results.iter().find(|(id, ..)| id == "d2").ok_or("d2")?;
    assert_eq!(d2.1, 2, "{}", d2.2);
    assert!(d2.2.contains("--- page 2 ---\nB two") && d2.2.contains("--- page 3 ---\nB three"));
    assert!(!d2.2.contains(NOT_SHOWN));
    let d1 = results.iter().find(|(id, ..)| id == "d1").ok_or("d1")?;
    assert_eq!(
        d1.1, 1,
        "one page of the older result fits the allowance: {}",
        d1.2
    );
    let t1 = results.iter().find(|(id, ..)| id == "t1").ok_or("t1")?;
    assert_eq!(t1.1, 0);
    assert!(!t1.2.contains(NOT_SHOWN));
    Ok(())
}

#[tokio::test]
async fn core_u_092_without_vision_the_text_goes_as_it_is() -> Result<(), Box<dyn Error>> {
    for (id, images, text) in results(&last_request(harness(false)?).await?) {
        assert_eq!(images, 0, "{id}");
        assert!(!text.contains(NOT_SHOWN), "{id}: {text}");
        if id.starts_with('d') {
            assert!(text.starts_with("[PDF, "), "{id}: {text}");
        }
    }
    Ok(())
}
