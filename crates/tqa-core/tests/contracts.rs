//! CORE-U-017/018: configuration and sink failure boundaries.
mod support;
use async_trait::async_trait;
use std::sync::Arc;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    CoreError, Decision, DecisionEngine, EngineFailure, ModelResponse, PlannedAction, RunOutcome,
    RunState, Runner,
};
use tqa_tools::ToolRegistry;
use tqa_types::ErrorCode;

#[tokio::test]
async fn core_u_038_idle_busy_and_invalid_inputs_never_write() -> TestResult {
    let h = Harness::new(vec![], ToolRegistry::new())?;
    let runner = Runner::new(h.deps)?;
    assert_eq!(
        runner
            .run(
                &[],
                None,
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Idle
    );
    let mut invalid = Some(turn());
    if let Some(message) = &mut invalid {
        message.text.clear();
    }
    assert_eq!(
        runner
            .run(
                &[],
                invalid,
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await
            .err()
            .ok_or("expected invalid turn")?
            .code(),
        ErrorCode::InvalidFrame
    );
    assert!(h.sink.frames().is_empty());
    Ok(())
}
#[tokio::test]
async fn core_u_005_sink_failure_stops_without_a_second_write() -> TestResult {
    for fail_at in 1..=9 {
        let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
        let mut sink = MemorySink::new(&[]);
        sink.fail_at = Some(fail_at);
        h.sink_with(sink);
        let result = Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await;
        assert!(matches!(result, Err(CoreError::Sink(_))));
        assert_eq!(h.sink.events().len(), fail_at - 1);
        assert_eq!(h.provider.active_streams(), 0);
    }
    Ok(())
}
struct Engine {
    invalid: bool,
}
#[async_trait]
impl DecisionEngine for Engine {
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        Ok({
            Decision::CallModel {
                route: state.route.clone(),
                tool_subset: state.tools.clone(),
            }
        })
    }

    async fn after_response(
        &self,
        _: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        Ok({
            if self.invalid {
                vec![PlannedAction {
                    call_id: "invented".into(),
                }]
            } else {
                response
                    .tool_calls
                    .iter()
                    .skip(1)
                    .map(|call| PlannedAction {
                        call_id: call.id.clone(),
                    })
                    .collect()
            }
        })
    }
}
#[tokio::test]
async fn core_u_032_engine_drop_is_answered_and_invalid_plan_falls_back() -> TestResult {
    for invalid in [false, true] {
        let tool = fake("read_file", true, vec![output("a", 0), output("b", 0)])?;
        let mut h = Harness::new(
            vec![
                calls(&[
                    ("a", "read_file", r#"{"path":"a"}"#),
                    ("b", "read_file", r#"{"path":"b"}"#),
                ]),
                answer(),
            ],
            registry(std::slice::from_ref(&tool))?,
        )?;
        h.deps.decision = Arc::new(Engine { invalid });
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        assert_eq!(tool.calls().len(), if invalid { 2 } else { 1 });
        if !invalid {
            assert!(tool_results(&h.sink.events()).iter().any(
                |(id, _, text)| id == "a" && text == "not run: skipped by the decision engine"
            ));
        }
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}
#[test]
fn core_u_018_constructor_validates_programmatic_limits() -> TestResult {
    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    h.deps.limits.max_steps = 0;
    assert!(Runner::new(h.deps).is_err());
    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    h.deps.route.max_output = Some(0);
    assert!(Runner::new(h.deps).is_err());
    Ok(())
}

#[tokio::test]
async fn core_u_026_sink_owns_the_first_sequence_base() -> TestResult {
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    let mut sink = MemorySink::new(&[]);
    sink.sequence_base = tqa_types::Seq::from(41);
    h.sink_with(sink);
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        h.sink.events()[1],
        tqa_types::Event::RunStarted {
            run_id: "r-42".parse()?
        }
    );
    assert_closed(&h.sink.frames())?;
    Ok(())
}
