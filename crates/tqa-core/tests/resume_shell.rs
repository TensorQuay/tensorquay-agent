//! Interrupted shell work is returned to the model before it explicitly chooses a new call.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_types::{ServerFrame, ToolStatus};

#[tokio::test]
async fn core_u_021_interrupted_shell_is_reported_before_the_model_retries_it() -> TestResult {
    let history = interrupted_shell().await?;
    let tool = fake("shell", false, vec![output("retried successfully", 0)])?;
    let mut h = Harness::new(
        vec![
            calls(&[("retry", "shell", r#"{"path":"action"}"#)]),
            answer(),
        ],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.sink_with(MemorySink::new(&history));
    assert_eq!(
        Runner::new(h.deps)?
            .run(
                &history,
                None,
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Completed
    );
    let requests = h.provider.requests();
    let interrupted = requests[0]
        .messages
        .last()
        .ok_or("missing interrupted result")?;
    assert_eq!(interrupted.tool_call_id.as_deref(), Some("original"));
    assert_eq!(
        interrupted.content,
        vec![tqa_llm::ContentPart::Text(
            "the sandbox stopped while this tool ran; it may be retried".into()
        )]
    );
    assert_eq!(tool.calls(), vec![serde_json::json!({"path":"action"})]);
    assert_eq!(
        tool_results(&h.sink.events()),
        vec![
            (
                "original".into(),
                ToolStatus::Interrupted,
                "the sandbox stopped while this tool ran; it may be retried".into()
            ),
            (
                "retry".into(),
                ToolStatus::Ok,
                "retried successfully".into()
            ),
        ]
    );
    assert_eq!(
        requests[1]
            .messages
            .last()
            .ok_or("missing retry result")?
            .tool_call_id
            .as_deref(),
        Some("retry")
    );
    assert_resumed(&history, &h.sink.frames())?;
    assert_closed(&h.sink.frames())?;
    Ok(())
}

async fn interrupted_shell() -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let tool = fake("shell", false, vec![output("unfinished", 20_000)])?;
    let h = Harness::new(
        vec![calls(&[("original", "shell", r#"{"path":"action"}"#)])],
        registry(std::slice::from_ref(&tool))?,
    )?;
    let shutdown = CancellationToken::new();
    let runner = Runner::new(h.deps)?;
    let run = runner.run(
        &[],
        Some(turn()),
        CancellationToken::new(),
        shutdown.clone(),
    );
    tokio::pin!(run);
    assert!(futures::poll!(&mut run).is_pending());
    for _ in 0..100 {
        if !tool.calls().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(tool.calls().len(), 1);
    shutdown.cancel();
    assert_eq!(run.await?, RunOutcome::Interrupted);
    assert!(tool_results(&h.sink.events()).is_empty());
    assert_eq!(h.provider.active_streams(), 0);
    Ok(h.sink.frames())
}
