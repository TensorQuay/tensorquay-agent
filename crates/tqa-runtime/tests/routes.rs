//! RT-I-014: route updates affect future runs without changing an active run or its journal.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use std::time::Duration;
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, ErrorCode, Event, ServerFrame};

fn second_session() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["route"]["name"] = json!("second");
    value["route"]["model"] = json!("second-model");
    value["route"]["relay_path"] = json!("/relay/r/second");
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

fn update() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let value = serde_json::to_value(second_session()?)?;
    Ok(ClientFrame::parse(&serde_json::to_vec(&json!({
        "type":"session.update", "route":value["route"]
    }))?)?)
}

fn tool_step(delay: Duration) -> Script {
    Script::new(vec![
        Scripted::Delay(delay),
        Scripted::ToolCall {
            name: "glob".into(),
            arguments_json: json!({"pattern":"*"}).to_string(),
            split_into: 1,
        },
    ])
}

fn completed(f: &Fixture) -> usize {
    f.gateway
        .stored()
        .iter()
        .filter(|frame| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::RunCompleted { .. },
                    ..
                }
            )
        })
        .count()
}

fn errors(f: &Fixture) -> Vec<ServerFrame> {
    f.gateway
        .frames()
        .into_iter()
        .filter(|frame| matches!(frame, ServerFrame::Error { .. }))
        .collect()
}

async fn action_round_trip(f: &Fixture) -> TestResult {
    let before = errors(f).len();
    f.gateway
        .send(ClientFrame::SessionInit {
            conversation_id: "barrier".parse()?,
            last_seq: tqa_types::Seq::ZERO,
            protocol: 1,
        })
        .await?;
    wait_for(|| errors(f).len() > before).await?;
    assert_eq!(errors(f).len(), before + 1);
    assert!(matches!(
        errors(f).last(),
        Some(ServerFrame::Error {
            seq: None,
            code: ErrorCode::InvalidFrame,
            ..
        })
    ));
    Ok(())
}

async fn update_without_events(f: &Fixture) -> TestResult {
    let before = f.gateway.stored();
    let before_errors = errors(f).len();
    f.gateway.send(update()?).await?;
    action_round_trip(f).await?;
    assert_eq!(errors(f).len(), before_errors + 1, "update was rejected");
    assert_eq!(f.gateway.stored(), before, "route update emitted an event");
    Ok(())
}

fn assert_routes(f: &Fixture, expected: &[(&str, &str)]) -> TestResult {
    let paths: Vec<_> = f
        .gateway
        .requests()
        .into_iter()
        .filter(|request| request.path.starts_with("/relay/"))
        .collect();
    let requests = f.gateway.model_requests();
    assert_eq!(paths.len(), expected.len());
    assert_eq!(requests.len(), expected.len());
    for ((path, request), (route, model)) in paths.iter().zip(&requests).zip(expected) {
        assert!(path.authenticated);
        assert_eq!(path.path, format!("/relay/r/{route}/chat/completions"));
        assert_eq!(request.body["model"], *model);
    }
    for (index, frame) in f.gateway.stored().iter().enumerate() {
        assert_eq!(serde_json::to_value(frame)?["seq"], index + 1);
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_014_between_runs_every_model_call_uses_the_updated_route() -> TestResult {
    let f = Fixture::in_memory(vec![answer(), tool_step(Duration::ZERO), answer()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    wait_for(|| completed(&f) == 1).await?;
    update_without_events(&f).await?;
    f.gateway.send(user()).await?;
    wait_for(|| completed(&f) == 2).await?;
    assert_routes(
        &f,
        &[
            ("local", "fake-model"),
            ("second", "second-model"),
            ("second", "second-model"),
        ],
    )?;
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_014_active_run_keeps_its_route_for_later_steps() -> TestResult {
    let delay = Duration::from_secs(2);
    let f = Fixture::in_memory(vec![tool_step(delay), answer(), answer()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    wait_for(|| f.sleeper.pending_durations().contains(&delay)).await?;
    f.wait_event("step.started").await?;
    update_without_events(&f).await?;
    assert_eq!(completed(&f), 0);
    f.clock.advance(delay)?;
    wait_for(|| completed(&f) == 1).await?;
    f.gateway.send(user()).await?;
    wait_for(|| completed(&f) == 2).await?;
    assert_routes(
        &f,
        &[
            ("local", "fake-model"),
            ("local", "fake-model"),
            ("second", "second-model"),
        ],
    )?;
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_014_reconnect_accepts_the_updated_snapshot_without_an_event() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    update_without_events(&f).await?;
    update_without_events(&f).await?;
    f.gateway.set_session(second_session()?)?;
    reconnect(&f).await?;
    assert!(f.gateway.stored().is_empty());
    f.gateway.send(user()).await?;
    wait_for(|| completed(&f) == 1).await?;
    assert_routes(&f, &[("second", "second-model")])?;
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

async fn reconnect(f: &Fixture) -> TestResult {
    f.gateway.disconnect();
    let delay = Duration::from_millis(125);
    wait_for(|| f.sleeper.pending_durations().contains(&delay)).await?;
    f.clock.advance(delay)?;
    f.gateway.wait_for_registrations(2).await?;
    action_round_trip(f).await
}

#[tokio::test]
async fn rt_i_014_missed_update_on_reconnect_changes_the_next_run_only() -> TestResult {
    for active in [false, true] {
        let delay = Duration::from_secs(2);
        let scripts = if active {
            vec![tool_step(delay), answer(), answer()]
        } else {
            vec![answer()]
        };
        let f = Fixture::in_memory(scripts).await?;
        f.gateway.set_bound(true);
        let (shutdown, task) = f.launch()?;
        f.gateway.wait_for_registrations(1).await?;
        action_round_trip(&f).await?;
        if active {
            f.gateway.send(user()).await?;
            wait_for(|| f.sleeper.pending_durations().contains(&delay)).await?;
            f.wait_event("step.started").await?;
        }
        let before = f.gateway.stored();
        f.gateway.set_session(second_session()?)?;
        reconnect(&f).await?;
        assert_eq!(f.gateway.stored(), before, "registration emitted an event");
        let mut expected = vec![];
        if active {
            f.clock.advance(delay)?;
            wait_for(|| completed(&f) == 1).await?;
            expected.extend([("local", "fake-model"); 2]);
        }
        f.gateway.send(user()).await?;
        wait_for(|| completed(&f) == usize::from(active) + 1).await?;
        expected.push(("second", "second-model"));
        assert_routes(&f, &expected)?;
        stop(shutdown, task).await?;
        f.gateway.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_014_route_refresh_keeps_every_other_registration_field_frozen() -> TestResult {
    for (field, value) in [
        ("conversation_id", json!("another")),
        ("protocol", json!(2)),
        (
            "limits",
            json!({"max_steps":2,"wall_clock_secs":600,"tool_result_bytes":8192}),
        ),
        ("workspace", json!({"display":"different"})),
        (
            "environment",
            json!({"template":"custom","digest":format!("sha256:{}", "a".repeat(64)),"capabilities":[]}),
        ),
        // A profile is frozen with the rest (ADR-0012).
        ("profile", json!({"name":"reviewer","skills":"none"})),
    ] {
        let f = Fixture::in_memory(vec![]).await?;
        f.gateway.set_bound(true);
        let (_, task) = f.launch()?;
        f.gateway.wait_for_registrations(1).await?;
        action_round_trip(&f).await?;
        let mut value_with_route = serde_json::to_value(second_session()?)?;
        value_with_route[field] = value;
        f.gateway
            .set_session(ClientFrame::parse(&serde_json::to_vec(&value_with_route)?)?)?;
        f.gateway.disconnect();
        let delay = Duration::from_millis(125);
        wait_for(|| f.sleeper.pending_durations().contains(&delay)).await?;
        f.clock.advance(delay)?;
        assert_eq!(
            tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task)
                .await??,
            Err(tqa_runtime::RuntimeError::SnapshotChanged(field)),
        );
        assert!(f.gateway.stored().is_empty());
        f.gateway.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_014_route_refresh_preserves_protocol_and_watermark_rejection() -> TestResult {
    for initial in [true, false] {
        let f = Fixture::in_memory(vec![]).await?;
        f.gateway.set_bound(true);
        let mut invalid = serde_json::to_value(second_session()?)?;
        invalid[if initial {
            "protocol"
        } else {
            "last_stored_seq"
        }] = json!(if initial { 2 } else { 1 });
        if initial {
            f.gateway.first_frame(serde_json::to_string(&invalid)?)?;
        }
        let (_, task) = f.launch()?;
        if !initial {
            f.gateway.wait_for_registrations(1).await?;
            action_round_trip(&f).await?;
            f.gateway.first_frame(serde_json::to_string(&invalid)?)?;
            f.gateway.disconnect();
            let delay = Duration::from_millis(125);
            wait_for(|| f.sleeper.pending_durations().contains(&delay)).await?;
            f.clock.advance(delay)?;
        }
        assert_eq!(
            tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task)
                .await??,
            Err(if initial {
                tqa_runtime::RuntimeError::Frame
            } else {
                tqa_runtime::RuntimeError::History
            }),
        );
        assert!(f.gateway.stored().is_empty());
        assert!(f.gateway.model_requests().is_empty());
        f.gateway.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_014_changed_route_on_restart_preserves_retained_history_limits() -> TestResult {
    for byte_limit in [false, true] {
        let f = Fixture::in_memory(vec![]).await?;
        f.gateway.set_bound(true);
        f.gateway.set_session(second_session()?)?;
        let frames = [
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: "question".into(),
                attachments: vec![],
            },
            Event::RunStarted {
                run_id: "active".parse()?,
            },
        ]
        .into_iter()
        .enumerate()
        .map(|(index, event)| ServerFrame::Event {
            seq: tqa_types::Seq::from(index as u64 + 1),
            run_id: None,
            step: None,
            ts: "2000-01-01T00:00:00Z".into(),
            event,
        })
        .collect::<Vec<_>>();
        let (mut wal, _) = tqa_context::Wal::open(&f.directory.path().join("state"))?;
        for frame in &frames {
            wal.append(frame)?;
        }
        wal.sync()?;
        drop(wal);
        let mut deps = f.deps()?;
        if byte_limit {
            deps.limits.history_bytes = serde_json::to_vec(&frames[0])?.len();
            deps.limits.history_page_bytes = deps.limits.history_bytes;
        } else {
            deps.limits.history_frames = 1;
            deps.limits.history_page = 1;
        }
        let runtime = tqa_runtime::Runtime::new(deps)?;
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(5),
                runtime.run(tokio_util::sync::CancellationToken::new()),
            )
            .await?,
            Err(tqa_runtime::RuntimeError::Limit),
        );
        let (_, retained) = tqa_context::Wal::open(&f.directory.path().join("state"))?;
        assert_eq!(retained.records.len(), 2);
        assert!(f.gateway.stored().is_empty());
        assert!(f.gateway.model_requests().is_empty());
        f.gateway.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_014_changed_route_cannot_discard_inconsistent_retained_run_history() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.set_bound(true);
    f.gateway.set_session(second_session()?)?;
    let orphaned_step = ServerFrame::Event {
        seq: 1.into(),
        run_id: Some("previous".parse()?),
        step: Some(1),
        ts: "2000-01-01T00:00:00Z".into(),
        event: Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
    };
    let (mut wal, _) = tqa_context::Wal::open(&f.directory.path().join("state"))?;
    wal.append(&orphaned_step)?;
    wal.sync()?;
    drop(wal);
    let (_, task) = f.launch()?;
    assert_eq!(
        tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await??,
        Err(tqa_runtime::RuntimeError::History)
    );
    assert!(f.gateway.model_requests().is_empty());
    assert!(!f.gateway.stored().iter().any(|frame| matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunStarted { .. } | Event::RunResumed { .. },
            ..
        }
    )));
    let (_, recovered) = tqa_context::Wal::open(&f.directory.path().join("state"))?;
    assert_eq!(recovered.records, vec![(1.into(), orphaned_step)]);
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_014_an_unknown_protocol_update_is_never_adopted_and_a_messages_update_is()
-> TestResult {
    let f = Fixture::in_memory(vec![answer(), answer(), answer()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    wait_for(|| completed(&f) == 1).await?;
    // Fail closed: an update this runtime cannot parse drops the link, the runtime redials and
    // receives its configuration again, and nothing reaches the relay.
    let mut unknown = serde_json::to_value(second_session()?)?;
    unknown["route"]["protocol"] = json!("future_protocol");
    let before = f.gateway.stored();
    let text = json!({"type":"session.update","route":unknown["route"]}).to_string();
    f.gateway.send_text(text).await?;
    let delay = Duration::from_millis(125);
    wait_for(|| f.sleeper.pending_durations().contains(&delay)).await?;
    f.clock.advance(delay)?;
    f.gateway.wait_for_registrations(2).await?;
    assert_eq!(f.gateway.stored(), before);
    assert_eq!(f.gateway.model_requests().len(), 1);
    f.gateway.send(user()).await?;
    wait_for(|| completed(&f) == 2).await?;
    // A Messages route is adopted by the next run and uses the Messages relay path.
    let mut messages = serde_json::to_value(second_session()?)?;
    messages["route"]["protocol"] = json!("anthropic_messages");
    let update = json!({"type":"session.update","route":messages["route"]});
    f.gateway
        .send(ClientFrame::parse(&serde_json::to_vec(&update)?)?)
        .await?;
    action_round_trip(&f).await?;
    f.gateway.send(user()).await?;
    wait_for(|| completed(&f) == 3).await?;
    let paths: Vec<_> = f
        .gateway
        .requests()
        .into_iter()
        .filter(|request| request.path.starts_with("/relay/"))
        .map(|request| request.path)
        .collect();
    assert_eq!(
        paths,
        [
            "/relay/r/local/chat/completions",
            "/relay/r/local/chat/completions",
            "/relay/r/second/messages"
        ]
    );
    let last = f.gateway.model_requests().pop().ok_or("no model request")?;
    assert_eq!(
        (last.path.as_str(), &last.body["model"]),
        ("/v1/messages", &json!("second-model"))
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}
