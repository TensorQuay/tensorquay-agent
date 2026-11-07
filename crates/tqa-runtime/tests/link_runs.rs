//! RT-I-001/002/003/004/007/038: real loopback link transport with injected run and reconnect clocks.
#[path = "support/fixture.rs"]
mod support;
use std::{sync::Arc, time::Duration};
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, ErrorCode, Event, ReasoningSnapshot, ServerFrame};

#[tokio::test]
async fn rt_i_001_pending_binding_redials_and_post_registration_revocation_exits() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    let (_, task) = f.launch()?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await?;
    assert_eq!(f.gateway.registrations(), 0);
    f.gateway.set_bound(true);
    f.clock.advance(Duration::from_millis(125))?;
    f.gateway.wait_for_registrations(1).await?;
    wait_for(|| {
        f.gateway
            .requests()
            .iter()
            .any(|r| r.path == "/runtime/history")
    })
    .await?;
    settle(&f).await?;
    f.gateway.revoke();
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await?;
    f.clock.advance(Duration::from_millis(125))?;
    tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await???;
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-038
/// A routine retire, as the gateway does it: the link's `restarting` error frame and close, then 401 on the redial
/// (its token is gone). The runtime ends cleanly, never with "runtime worker failed".
#[tokio::test]
async fn rt_i_038_a_routine_retire_ends_the_runtime_cleanly() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    f.gateway.set_bound(true);
    let (_, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    wait_for(|| {
        f.gateway
            .requests()
            .iter()
            .any(|r| r.path == "/runtime/history")
    })
    .await?;
    settle(&f).await?;
    let retired = tqa_types::ServerFrame::Error {
        target: None,
        seq: None,
        code: tqa_types::ErrorCode::Restarting,
        message: "restarting".into(),
        retryable: true,
    };
    f.gateway
        .send_text(serde_json::to_string(&retired)?)
        .await?;
    f.gateway.revoke();
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await?;
    f.clock.advance(Duration::from_millis(125))?;
    let exit =
        tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await??;
    assert!(
        exit.is_ok(),
        "a routine retire is a clean stop, not {exit:?}"
    );
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-038
/// The same retire on threads, as in production, held at the moment that matters: the link has let go of its
/// channels but its task hasn't ended. The supervisor must wait for how the link ended; taking the closed channel
/// for a failed worker is what made every routine microVM stop log "runtime worker failed".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_i_038_a_link_that_let_go_of_its_channels_is_not_a_failed_worker() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    f.gateway.set_bound(true);
    let hooks = tqa_runtime::testing::TestingHooks::default();
    hooks.link_finished.pause();
    let (_, task) = f.launch_with(hooks.clone())?;
    f.gateway.wait_for_registrations(1).await?;
    wait_for(|| {
        f.gateway
            .requests()
            .iter()
            .any(|r| r.path == "/runtime/history")
    })
    .await?;
    settle(&f).await?;
    let retired = ServerFrame::Error {
        target: None,
        seq: None,
        code: ErrorCode::Restarting,
        message: "restarting".into(),
        retryable: true,
    };
    f.gateway
        .send_text(serde_json::to_string(&retired)?)
        .await?;
    f.gateway.revoke();
    // Redial after each reconnect wait (on threads a redial can need more than one) until the link has ended. Once
    // the supervisor waits for how the link ended, the link has let go of its channels and needs no more redials: a
    // jump then would pass the supervisor's own bound and fail the retire as a timeout, which is not what's tested.
    let deadline =
        std::time::Instant::now() + tqa_testkit::deadline::scaled(Duration::from_secs(5));
    while !hooks.link_finished.entered() {
        assert!(
            std::time::Instant::now() < deadline,
            "the link never ended after the retire"
        );
        // Only ever a jump shorter than that bound: the supervisor may start its wait between the check and the jump.
        if !f.sleeper.pending_named(tqa_runtime::testing::LINK_END_WAIT)
            && let Some(wait) = f
                .sleeper
                .pending_durations()
                .into_iter()
                .filter(|wait| *wait < tqa_runtime::RuntimeLimits::RECOMMENDED.io_timeout)
                .min()
        {
            f.clock.advance(wait)?;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Real time on purpose: a bounded window for the supervisor, on its own thread, to react to the closed channels.
    // Stopping the link now would mean it took them for a failure before knowing how the link ended.
    let stopped_early = tokio::time::timeout(Duration::from_secs(1), async {
        while !hooks
            .link_stopping
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    hooks.link_finished.resume();
    let exit =
        tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await??;
    assert!(
        !stopped_early,
        "the supervisor stopped before the link had ended"
    );
    assert!(
        exit.is_ok(),
        "a routine retire is a clean stop, not {exit:?}"
    );
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_003_busy_message_never_starts_a_second_run() -> TestResult {
    let f = Fixture::new(vec![Script::new(vec![
        Scripted::Text("part".into()),
        Scripted::Delay(Duration::from_secs(10)),
        Scripted::Text("end".into()),
    ])])
    .await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    f.gateway.send(user()).await?;
    wait_for(|| {
        f.gateway.frames().iter().any(|frame| {
            matches!(
                frame,
                ServerFrame::Error {
                    seq: None,
                    code: ErrorCode::Busy,
                    retryable: true,
                    ..
                }
            )
        })
    })
    .await?;
    settle(&f).await?;
    f.clock.advance(Duration::from_secs(10))?;
    f.wait_event("run.completed").await?;
    assert_eq!(
        f.gateway
            .stored()
            .iter()
            .filter(|frame| matches!(
                frame,
                ServerFrame::Event {
                    event: Event::MessageUser { .. },
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(f.gateway.model_requests().len(), 1);
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_007_relay_only_authentication_and_canonical_reasoning_field() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    let mut configuration = session()?;
    if let ClientFrame::SessionConfig { route, .. } = &mut configuration {
        route.reasoning = Some(ReasoningSnapshot {
            allowed: vec!["low".into()],
            default: "low".into(),
            budget_tokens: None,
        });
    }
    f.gateway.set_session(configuration)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    std::fs::remove_file(f.directory.path().join("credential"))?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    let requests = f.gateway.requests();
    assert!(requests.iter().all(|request| request.authenticated));
    assert!(requests.iter().all(|request| matches!(
        request.path.as_str(),
        // The channel memberships and the agent-team role are asked at run start (ADR-0014, RT-I-030; ADR-0034),
        // with the same runtime token.
        "/runtime/link"
            | "/runtime/history"
            | "/runtime/channels"
            | "/runtime/teams/role"
            | "/relay/r/local/chat/completions"
    )));
    assert_eq!(
        f.gateway.model_requests()[0].body["reasoning_effort"],
        "low"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_002_link_outage_keeps_running_and_resends_the_durable_tail() -> TestResult {
    let f = Fixture::new(vec![Script::new(vec![
        Scripted::Text("part".into()),
        Scripted::Delay(Duration::from_secs(1)),
        Scripted::Text("end".into()),
    ])])
    .await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    settle(&f).await?;
    f.gateway.set_persist(false);
    f.gateway.disconnect();
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await?;
    f.clock.advance(Duration::from_secs(1))?;
    wait_for(|| {
        std::fs::read_to_string(f.directory.path().join("state/events.jsonl"))
            .is_ok_and(|log| log.contains("run.completed"))
    })
    .await?;
    f.gateway.wait_for_registrations(2).await?;
    f.wait_event("run.completed").await?;
    settle(&f).await?;
    f.gateway.set_persist(true);
    f.gateway.disconnect();
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await?;
    f.clock.advance(Duration::from_millis(125))?;
    wait_for(|| {
        f.gateway.stored().iter().any(|frame| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::RunCompleted { .. },
                    ..
                }
            )
        })
    })
    .await?;
    let stored = f.gateway.stored();
    assert!(matches!(
        stored.last(),
        Some(ServerFrame::Event {
            event: Event::RunCompleted { .. },
            ..
        })
    ));
    for (index, frame) in stored.iter().enumerate() {
        assert_eq!(serde_json::to_value(frame)?["seq"], index + 1);
    }
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_004_heartbeats_come_only_from_an_active_unstalled_supervisor() -> TestResult {
    let mut events = vec![Scripted::Text("start".into())];
    for _ in 0..12 {
        events.extend([
            Scripted::Delay(Duration::from_secs(15)),
            Scripted::Text("tick".into()),
        ]);
    }
    let f = Fixture::new(vec![Script::new(events)]).await?;
    f.gateway.set_bound(true);
    let deps = f.deps()?;
    let gate = Arc::clone(&deps.testing.supervisor);
    let runtime = tqa_runtime::Runtime::new(deps)?;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let token = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(token).await });
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    for tick in 1..=2 {
        advance_tick(&f, tick).await?;
    }
    wait_for(|| heartbeats(&f) == 1).await?;
    gate.pause();
    f.gateway
        .send(ClientFrame::RunCancel {
            run_id: "unmatched".parse()?,
        })
        .await?;
    gate.wait_until_entered().await?;
    for tick in 3..=4 {
        advance_tick(&f, tick).await?;
    }
    assert_eq!(
        heartbeats(&f),
        1,
        "the stalled supervisor produced a heartbeat"
    );
    assert_eq!(
        f.gateway.registrations(),
        1,
        "the socket must remain open during the stall"
    );
    gate.resume();
    wait_for(|| heartbeats(&f) == 2).await?;
    for tick in 5..=12 {
        advance_tick(&f, tick).await?;
    }
    f.wait_event("run.completed").await?;
    assert!(
        heartbeats(&f) >= 5,
        "missing a thirty-second active heartbeat"
    );
    settle(&f).await?;
    let count = heartbeats(&f);
    f.clock.advance(Duration::from_mins(1))?;
    // A supervisor action round trip proves the idle timer has been polled.
    f.gateway
        .send(ClientFrame::SessionInit {
            conversation_id: "request".parse()?,
            last_seq: tqa_types::Seq::ZERO,
            protocol: 1,
        })
        .await?;
    wait_for(|| {
        f.gateway.frames().iter().any(|f| {
            matches!(
                f,
                ServerFrame::Error {
                    code: ErrorCode::InvalidFrame,
                    ..
                }
            )
        })
    })
    .await?;
    assert_eq!(heartbeats(&f), count);
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}
fn heartbeats(f: &Fixture) -> usize {
    f.gateway
        .frames()
        .iter()
        .filter(|f| matches!(f, ServerFrame::Heartbeat { .. }))
        .count()
}
async fn advance_tick(f: &Fixture, tick: usize) -> TestResult {
    settle(f).await?;
    f.clock.advance(Duration::from_secs(15))?;
    wait_for(|| {
        f.gateway
            .frames()
            .iter()
            .filter(|f| {
                matches!(
                    f,
                    ServerFrame::Event {
                        event: Event::TextDelta { .. },
                        ..
                    }
                )
            })
            .count()
            == tick + 1
    })
    .await?;
    Ok(())
}
async fn settle(f: &Fixture) -> TestResult {
    wait_for(|| {
        !f.sleeper
            .pending_durations()
            .contains(&Duration::from_secs(5))
    })
    .await
}
