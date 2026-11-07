//! LLM-U-015 at the loop boundary: every model request asks for the route's default reasoning effort, and a route
//! without reasoning asks for none.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::Runner;
use tqa_tools::ToolRegistry;
use tqa_types::ReasoningSnapshot;

async fn requested(
    reasoning: Option<ReasoningSnapshot>,
) -> Result<Vec<Option<String>>, Box<dyn std::error::Error>> {
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.deps.route.reasoning = reasoning;
    let provider = h.provider.clone();
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    Ok(provider
        .requests()
        .into_iter()
        .map(|request| request.reasoning_effort)
        .collect())
}

#[tokio::test]
async fn llm_u_015_the_loop_requests_the_route_default_effort() -> TestResult {
    let reasoning = ReasoningSnapshot {
        allowed: vec!["low".into(), "high".into()],
        default: "high".into(),
        budget_tokens: None,
    };
    assert_eq!(requested(Some(reasoning)).await?, [Some("high".to_owned())]);
    assert_eq!(requested(None).await?, [None]);
    Ok(())
}
