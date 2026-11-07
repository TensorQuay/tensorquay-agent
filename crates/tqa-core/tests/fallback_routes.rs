//! CORE-U-041 / CORE-U-042 / CORE-U-043: a run falls back to the next admin-declared route.
mod support;
use serde_json::json;
use std::{error::Error, sync::Arc, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RouteProvider, RunOutcome, Runner, UserTurn};
use tqa_llm::{ContentPart, LlmError, Registry, StreamChunk};
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event, RouteSnapshot, ServerFrame};

/// The fallback route's snapshot; tests adjust its capabilities.
fn backup_route() -> RouteSnapshot {
    RouteSnapshot {
        name: "backup".into(),
        relay_path: "/relay/r/backup".into(),
        model: "backup-model".into(),
        ..route()
    }
}
fn backup_provider(
    h: &Harness,
    script: Vec<ProviderReply>,
) -> Result<Arc<FakeProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(
        "[[routes]]\nname='backup'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='backup-model'\nauth={kind='none'}\n",
    )?;
    Ok(Arc::new(FakeProvider::new(
        routes.get("backup").ok_or("missing route")?.clone(),
        script,
        Arc::new(h.sleeper.clone()),
    )))
}
fn with_backup(
    h: &mut Harness,
    snapshot: RouteSnapshot,
    script: Vec<ProviderReply>,
) -> Result<Arc<FakeProvider>, Box<dyn Error>> {
    let provider = backup_provider(h, script)?;
    h.deps.fallback_routes = vec![RouteProvider {
        route: snapshot,
        provider: provider.clone(),
    }];
    Ok(provider)
}
fn rate_limited(seconds: u64) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Err(LlmError::RateLimited {
            retry_after: Some(Duration::from_secs(seconds)),
        }),
    }
}
fn switches(events: &[Event]) -> Vec<(String, String, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::RouteFallback { from, to, reason } => {
                Some((from.clone(), to.clone(), reason.clone()))
            }
            _ => None,
        })
        .collect()
}
fn system_text(request: &tqa_llm::CompletionRequest) -> String {
    request.messages[0]
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_u_041_exhausted_retries_switch_once_and_the_run_stays_there() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let mut h = Harness::new(
        vec![http(503), http(503), http(503), answer()],
        registry(&[tool])?,
    )?;
    let backup = with_backup(
        &mut h,
        backup_route(),
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
    )?;
    let (primary, sink) = (h.provider.clone(), h.sink.clone());
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        primary.requests().len(),
        3,
        "the primary used its whole retry budget"
    );
    assert_eq!(
        backup.requests().len(),
        2,
        "both steps of the run stayed on the fallback"
    );
    assert_eq!(
        switches(&sink.events()),
        vec![("test".into(), "backup".into(), "http 503".into())]
    );
    // The next run starts on the conversation's own route again.
    let history = sink.frames();
    let second = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "again".into(),
        attachments: vec![],
    };
    let outcome = drive(
        runner.run(
            &history,
            Some(second),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(primary.requests().len(), 4);
    assert_eq!(backup.requests().len(), 2);
    Ok(())
}

#[tokio::test]
async fn core_u_041_a_long_retry_after_switches_at_once() -> TestResult {
    let mut h = Harness::new(vec![rate_limited(90)], ToolRegistry::new())?;
    let backup = with_backup(&mut h, backup_route(), vec![answer()])?;
    let (primary, sink) = (h.provider.clone(), h.sink.clone());
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        primary.requests().len(),
        1,
        "no waiting out a 90 s Retry-After"
    );
    assert_eq!(backup.requests().len(), 1);
    assert_eq!(
        switches(&sink.events()),
        vec![("test".into(), "backup".into(), "rate_limited".into())]
    );
    Ok(())
}

#[tokio::test]
async fn core_u_042_permanent_errors_do_not_switch_and_the_last_route_ends_the_run() -> TestResult {
    let mut h = Harness::new(vec![http(400)], ToolRegistry::new())?;
    let backup = with_backup(&mut h, backup_route(), vec![answer()])?;
    let sink = h.sink.clone();
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert!(matches!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            ..
        }
    ));
    assert!(backup.requests().is_empty());
    assert!(switches(&sink.events()).is_empty());
    let mut h = Harness::new(vec![http(503), http(503), http(503)], ToolRegistry::new())?;
    let backup = with_backup(
        &mut h,
        backup_route(),
        vec![http(502), http(502), http(502)],
    )?;
    let sink = h.sink.clone();
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert!(matches!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            ..
        }
    ));
    assert_eq!(backup.requests().len(), 3);
    assert_eq!(switches(&sink.events()).len(), 1);
    Ok(())
}

#[tokio::test]
async fn core_u_043_a_fallback_without_native_tools_uses_the_text_protocol() -> TestResult {
    let mut h = Harness::new(vec![rate_limited(90)], ToolRegistry::new())?;
    let backup = with_backup(
        &mut h,
        RouteSnapshot {
            native_tools: false,
            ..backup_route()
        },
        vec![answer()],
    )?;
    let primary = h.provider.clone();
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    assert!(!system_text(&primary.requests()[0]).contains(tqa_prompt::FALLBACK_INSTRUCTIONS));
    let request = &backup.requests()[0];
    assert!(request.tools.is_empty());
    assert!(system_text(request).contains(tqa_prompt::FALLBACK_INSTRUCTIONS));
    Ok(())
}

/// About 20 * 330 tokens of history: it fits the primary's 32k window but not 85 % of 8k.
fn long_history() -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let mut events = Vec::new();
    for index in 1..=20_u64 {
        let seq = u64::try_from(events.len())? + 1;
        let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
        events.extend([
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: format!("request {index} {}", "x".repeat(1100)),
                attachments: vec![],
            },
            Event::RunStarted {
                run_id: run_id.clone(),
            },
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
            Event::TextDelta {
                text: format!("answer {index}"),
            },
            Event::ModelCompleted {
                step: 1,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::Stop,
            },
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
            Event::RunCompleted { run_id },
        ]);
    }
    history(events)
}

#[tokio::test]
async fn core_u_043_a_smaller_fallback_window_compacts_before_its_first_request() -> TestResult {
    let history = long_history()?;
    let mut h = Harness::new(vec![rate_limited(90)], ToolRegistry::new())?;
    h.deps.limits.max_output_tokens = 1_000_000;
    let summary = serde_json::to_string(&json!({"goal":"g","done":[],"pending":[],"decisions":[],
        "key_paths":[],"open_errors":[]}))?;
    let backup = with_backup(
        &mut h,
        RouteSnapshot {
            context_window: Some(8192),
            max_output: Some(1024),
            ..backup_route()
        },
        vec![
            reply(vec![
                StreamChunk::TextDelta(summary),
                usage(),
                StreamChunk::Finish(tqa_llm::FinishReason::Stop),
            ]),
            answer(),
        ],
    )?;
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &history,
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let types = sink.types();
    let switched = types
        .iter()
        .position(|kind| kind == "route.fallback")
        .ok_or("no switch")?;
    let compacted = types
        .iter()
        .position(|kind| kind == "compaction")
        .ok_or("no compaction")?;
    assert!(
        switched < compacted,
        "the window is re-checked on the new route"
    );
    let requests = backup.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].tools.is_empty(),
        "the first backup request is the summary call"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_043_a_fallback_without_vision_sends_the_image_notice() -> TestResult {
    let mut h = Harness::new(vec![rate_limited(90)], ToolRegistry::new())?;
    h.deps.route.vision = true;
    std::fs::create_dir_all(h.directory.path().join("uploads"))?;
    std::fs::write(
        h.directory.path().join("uploads/a.png"),
        b"\x89PNG\r\n\x1a\nimage",
    )?;
    let backup = with_backup(&mut h, backup_route(), vec![answer()])?;
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "look".into(),
        attachments: vec!["uploads/a.png".into()],
    };
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &[],
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let request = &backup.requests()[0];
    let user = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == tqa_llm::Role::User)
        .ok_or("no user message")?;
    assert!(
        user.content
            .iter()
            .all(|part| matches!(part, ContentPart::Text(_)))
    );
    let text: String = user
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.ends_with(
        "\n[Images not sent, data only: \"/workspace/uploads/a.png\": no image input on this route]"
    ));
    Ok(())
}

/// A run in progress whose journal already switched to `to`.
fn switched_history(to: &str) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    history(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "task".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r-1".parse()?,
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::RouteFallback {
            from: "test".into(),
            to: to.into(),
            reason: "http 503".into(),
        },
    ])
}

#[tokio::test]
async fn core_u_043_resume_continues_on_the_recorded_route() -> TestResult {
    let history = switched_history("backup")?;
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    let backup = with_backup(&mut h, backup_route(), vec![answer()])?;
    h.sink_with(MemorySink::new(&history));
    let primary = h.provider.clone();
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(backup.requests().len(), 1);
    assert!(primary.requests().is_empty());
    Ok(())
}

#[tokio::test]
async fn core_u_043_a_vanished_fallback_resumes_on_the_own_route_with_a_notice() -> TestResult {
    let history = switched_history("gone")?;
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    let backup = with_backup(&mut h, backup_route(), vec![answer()])?;
    h.sink_with(MemorySink::new(&history));
    let (primary, sink) = (h.provider.clone(), h.sink.clone());
    let outcome = drive(
        Runner::new(h.deps)?.run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(primary.requests().len(), 1);
    assert!(backup.requests().is_empty());
    assert_eq!(
        switches(&sink.events())[1..],
        [("gone".into(), "test".into(), "route_unavailable".into())]
    );
    Ok(())
}
