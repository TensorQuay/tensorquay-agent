//! RT-I-032 (ADR-0013 points 1 and 7, revision 9): the runtime consults a decision engine only when session.config
//! names one. Without one the model decides: no consultation and no decision frame. With one it posts to the engine's
//! relay path with its own token and journals the decision it applied.
#[path = "support/fixture.rs"]
mod support;
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{
    ClientFrame, DecisionEngineSnapshot, DecisionExposure, DecisionPoint, Event, ServerFrame,
    decision_v1,
};

fn decisions(frames: &[ServerFrame]) -> Vec<Event> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event { event, .. }
                if matches!(
                    event,
                    Event::Decision { .. } | Event::DecisionFallback { .. }
                ) =>
            {
                Some(event.clone())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn rt_i_032_without_an_engine_the_model_decides() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.set_session(session()?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    assert_eq!(decisions(&f.gateway.frames()), [], "no decision frame");
    assert!(
        !f.gateway
            .requests()
            .iter()
            .any(|request| request.path.starts_with("/relay/d/")),
        "no consultation"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_032_a_named_engine_is_consulted_over_its_relay_path() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    let mut configuration = session()?;
    if let ClientFrame::SessionConfig {
        decision_engine, ..
    } = &mut configuration
    {
        *decision_engine = Some(DecisionEngineSnapshot {
            name: "router".into(),
            relay_path: DecisionEngineSnapshot::relay_path_for("router"),
            timeout_ms: 2000,
            exposure: DecisionExposure::None,
            points: None,
        });
    }
    f.gateway.set_session(configuration)?;
    f.gateway
        .set_decisions(vec![decision_v1::Response::CallModel {
            route: "local".into(),
            tools: vec![],
        }]);
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    let consultations: Vec<_> = f
        .gateway
        .requests()
        .into_iter()
        .filter(|request| request.path.starts_with("/relay/d/"))
        .collect();
    assert_eq!(
        consultations.first().map(|request| request.path.as_str()),
        Some("/relay/d/router/v1/before_step")
    );
    assert!(consultations.iter().all(|request| request.authenticated));
    let journaled = decisions(&f.gateway.frames());
    assert!(
        matches!(
            journaled.first(),
            Some(Event::Decision { engine, .. }) if engine == "router"
        ),
        "{journaled:?}"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

/// A run whose model calls a tool and then answers, with an engine named in session.config; the relay paths asked.
async fn consulted(
    points: Option<Vec<DecisionPoint>>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let calls = Script::new(vec![Scripted::ToolCall {
        name: "read_file".into(),
        arguments_json: r#"{"path":"notes.txt"}"#.into(),
        split_into: 1,
    }]);
    let f = Fixture::in_memory(vec![calls, answer()]).await?;
    let mut configuration = session()?;
    if let ClientFrame::SessionConfig {
        decision_engine, ..
    } = &mut configuration
    {
        *decision_engine = Some(DecisionEngineSnapshot {
            name: "router".into(),
            relay_path: DecisionEngineSnapshot::relay_path_for("router"),
            timeout_ms: 2000,
            exposure: DecisionExposure::None,
            points,
        });
    }
    f.gateway.set_session(configuration)?;
    let call_model = || decision_v1::Response::CallModel {
        route: "local".into(),
        tools: vec![],
    };
    f.gateway.set_decisions(vec![call_model(), call_model()]);
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    let asked: Vec<String> = f
        .gateway
        .requests()
        .into_iter()
        .filter(|request| request.path.starts_with("/relay/d/"))
        .map(|request| request.path)
        .collect();
    let after_records = decisions(&f.gateway.frames())
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::Decision {
                    point: DecisionPoint::AfterResponse,
                    ..
                } | Event::DecisionFallback {
                    point: DecisionPoint::AfterResponse,
                    ..
                }
            )
        })
        .count();
    let asked_after = asked.iter().any(|path| path.ends_with("/after_response"));
    assert_eq!(
        after_records > 0,
        asked_after,
        "an after_response record exactly when it was asked"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(asked)
}

#[tokio::test]
async fn rt_i_032_an_engine_is_asked_only_at_the_points_it_answers() -> TestResult {
    let asks = |asked: &[String], point: &str| asked.iter().any(|path| path.ends_with(point));
    // Absent: both points, as for every decision/v1 engine.
    let both = consulted(None).await?;
    assert!(
        asks(&both, "/before_step") && asks(&both, "/after_response"),
        "{both:?}"
    );
    // Before a step only: never asked after a response, and nothing journaled for it.
    let before = consulted(Some(vec![DecisionPoint::BeforeStep])).await?;
    assert!(
        asks(&before, "/before_step") && !asks(&before, "/after_response"),
        "{before:?}"
    );
    // After a response only: never asked before a step.
    let after = consulted(Some(vec![DecisionPoint::AfterResponse])).await?;
    assert!(
        !asks(&after, "/before_step") && asks(&after, "/after_response"),
        "{after:?}"
    );
    Ok(())
}
