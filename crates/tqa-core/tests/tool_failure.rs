//! CORE-U-076 (C1): a refused tool call reaches the model in plain words, and its failure reaches clients as data: the
//! `tool.end` of a call the proxy refused carries the code `egress_denied` and the destination, so a client words it
//! itself without reading the model's text.
mod support;
use std::{error::Error, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_testkit::FakeToolReply;
use tqa_tools::ToolError;
use tqa_types::{ErrorTarget, Event, ToolFailure, ToolStatus};

// TEST: CORE-U-076
#[tokio::test]
async fn core_u_076_a_refused_call_ends_with_its_failure_and_the_model_reads_plain_words()
-> Result<(), Box<dyn Error>> {
    let target = ErrorTarget::new("example.com", 8443)?;
    let tool = fake(
        "fetch_page",
        false,
        vec![FakeToolReply {
            delay: Duration::ZERO,
            result: Err(ToolError::EgressDenied {
                target: target.clone(),
            }),
        }],
    )?;
    let script = vec![calls(&[("c1", "fetch_page", "{\"path\":\"x\"}")]), answer()];
    let h = Harness::new(script, registry(&[tool])?)?;
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
    let events = sink.events();
    let failures: Vec<Option<ToolFailure>> = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolEnd { failure, .. } => Some(failure.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        failures,
        [Some(ToolFailure::new("egress_denied", Some(target))?)]
    );
    let results = tool_results(&events);
    assert_eq!(results.len(), 1);
    let (_, status, content) = &results[0];
    assert_eq!(*status, ToolStatus::Error);
    assert_eq!(
        content,
        "example.com (port 8443) isn't on this task's allowed sites. An administrator can allow it in Settings > \
         Internet access."
    );
    Ok(())
}

/// A `computer` refusal's own code (`stale_ref`, `screenshot_limit`, `action_limit`, `screenshot_too_large`) is its
/// `tool.end` failure's code, not a catch-all, since a client words the step from it.
// TEST: CORE-U-076
#[tokio::test]
async fn core_u_076_a_screen_refusal_ends_with_its_own_code() -> Result<(), Box<dyn Error>> {
    let tool = fake(
        "computer",
        false,
        vec![FakeToolReply {
            delay: Duration::ZERO,
            result: Err(ToolError::screen(
                "screenshot_too_large",
                "the screen has too much detail to send as a picture even at its smallest size; work from observe",
            )),
        }],
    )?;
    let script = vec![calls(&[("c1", "computer", "{\"path\":\"x\"}")]), answer()];
    let h = Harness::new(script, registry(&[tool])?)?;
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
    let failures: Vec<Option<ToolFailure>> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::ToolEnd { failure, .. } => Some(failure.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        failures,
        [Some(ToolFailure::new("screenshot_too_large", None)?)]
    );
    Ok(())
}
