//! TOOL-U-011: finite fake scripts, injected latency and cancellation.
use serde_json::json;
use std::{error::Error, sync::Arc, time::Duration};
use tqa_testkit::{FakeClock, FakeSleeper, FakeTool, FakeToolReply, workspace_fixture};
use tqa_tools::{Tool, ToolContext, ToolError, ToolLimits, ToolName, ToolOutput, ToolSpec};

#[tokio::test]
async fn tool_u_011_fake_latency_failure_exhaustion_and_cancel() -> Result<(), Box<dyn Error>> {
    let fixture = workspace_fixture()?;
    assert!(fixture.root().join("binary.bin").exists());
    assert!(fixture.outside().join("secret.txt").exists());
    let clock = FakeClock::new();
    let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
    let cx = ToolContext {
        workspace: fixture.workspace().clone(),
        cancel: tokio_util::sync::CancellationToken::new(),
        clock: Arc::new(clock.clone()),
        sleeper: sleeper.clone(),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    };
    let spec = ToolSpec {
        name: ToolName::new("fake")?,
        description: "Scripted test".into(),
        parameters: json!({}),
        parallel_safe: true,
        changes_workspace: false,
    };
    let fake = FakeTool::new(
        spec,
        vec![
            FakeToolReply {
                delay: Duration::from_secs(5),
                result: Ok(ToolOutput::text("ok")),
            },
            FakeToolReply {
                delay: Duration::ZERO,
                result: Err(ToolError::NotFound),
            },
            FakeToolReply {
                delay: Duration::from_secs(5),
                result: Ok(ToolOutput::text("late")),
            },
        ],
    );
    let mut call = Box::pin(fake.call(json!({"first":1}), &cx));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(call.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    clock.advance(Duration::from_secs(5))?;
    assert_eq!(call.await?.text, "ok");
    assert!(matches!(
        fake.call(json!({}), &cx).await,
        Err(ToolError::NotFound)
    ));
    let mut call = Box::pin(fake.call(json!({}), &cx));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(call.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    cx.cancel.cancel();
    assert!(matches!(call.await, Err(ToolError::Cancelled)));
    assert!(matches!(
        fake.call(json!({}), &cx).await,
        Err(ToolError::Cancelled)
    ));
    let mut fresh = cx.clone();
    fresh.cancel = tokio_util::sync::CancellationToken::new();
    assert!(matches!(
        fake.call(json!({}), &fresh).await,
        Err(ToolError::Unsupported(_))
    ));
    assert_eq!(fake.calls().len(), 3);
    Ok(())
}
