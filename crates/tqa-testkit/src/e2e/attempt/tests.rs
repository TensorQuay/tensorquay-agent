use super::*;
use crate::e2e::manifest::{Budgets, Check, CheckImage};
use axum::{
    Router,
    body::Body,
    extract::Query,
    http::Uri,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex, PoisonError},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const CONVERSATION: &str = "0192a7d4-0000-7000-8000-000000000001";

/// One SSE response: its frames, then either the end of the body or a stream that stays open.
struct Script {
    frames: Vec<ServerFrame>,
    hang: bool,
}
#[derive(Default)]
struct Stand {
    scripts: Vec<Script>,
    journal: Vec<ServerFrame>,
    requests: Vec<String>,
}

fn event(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-09-26T00:00:00Z".into(),
        event,
    }
}
fn run_id() -> Result<RunId, serde_json::Error> {
    serde_json::from_value(json!("r-1"))
}
fn usage(seq: u64, tokens: u64) -> ServerFrame {
    event(
        seq,
        Event::Usage {
            prompt_tokens: tokens,
            completion_tokens: 0,
            cached_tokens: None,
            reasoning_tokens: None,
            estimated: false,
            estimated_input: None,
            estimated_images: None,
            estimated_route: None,
        },
    )
}

/// The stand-in's answer to one request, by path.
fn respond(stand: &mut Stand, uri: &Uri, query: &HashMap<String, String>) -> Response {
    let streaming = query.contains_key("stream");
    if uri.path() == "/api/v1/conversations" {
        return axum::Json(json!({ "id": CONVERSATION })).into_response();
    }
    if uri.path().ends_with("/events") && streaming {
        // Like the gateway, a stream without the wire protocol version is refused.
        if query.get("protocol") != Some(&tqa_types::PROTOCOL_VERSION.to_string()) {
            return axum::http::StatusCode::BAD_REQUEST.into_response();
        }
        return if stand.scripts.is_empty() {
            axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response()
        } else {
            sse(&stand.scripts.remove(0))
        };
    }
    if uri.path().ends_with("/events") {
        let page = json!({ "items": stand.journal, "next_after": stand.journal.len() });
        return axum::Json(page).into_response();
    }
    axum::http::StatusCode::ACCEPTED.into_response()
}

fn sse(script: &Script) -> Response {
    let mut body = String::new();
    for data in script
        .frames
        .iter()
        .filter_map(|frame| serde_json::to_string(frame).ok())
    {
        body.push_str("data: ");
        body.push_str(&data);
        body.push_str("\n\n");
    }
    let head = futures::stream::iter([Ok::<_, Infallible>(body)]);
    let body = if script.hang {
        Body::from_stream(futures::StreamExt::chain(head, futures::stream::pending()))
    } else {
        Body::from_stream(head)
    };
    ([("content-type", "text/event-stream")], body).into_response()
}

async fn stand(stand: Stand) -> Result<(String, Arc<Mutex<Stand>>), std::io::Error> {
    let shared = Arc::new(Mutex::new(stand));
    let state = shared.clone();
    let app = Router::new().fallback(
        move |uri: Uri, Query(query): Query<HashMap<String, String>>| {
            let state = state.clone();
            async move {
                let mut stand = state.lock().unwrap_or_else(PoisonError::into_inner);
                stand.requests.push(uri.to_string());
                respond(&mut stand, &uri, &query)
            }
        },
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((format!("http://{address}"), shared))
}

fn manifest(max_tokens: u64, wall_clock_secs: u32) -> Manifest {
    Manifest {
        id: "py-fix".into(),
        title: "t".into(),
        tags: vec![],
        budgets: Budgets {
            max_steps: 30,
            wall_clock_secs,
            max_tokens,
        },
        protected: vec![],
        check: Check {
            image: CheckImage::Runtime,
            timeout_secs: 60,
        },
        expects_compaction: false,
        template: None,
    }
}

async fn attempt(
    scripts: Vec<Script>,
    journal: Vec<ServerFrame>,
    manifest: &Manifest,
) -> Result<(Followed, Vec<String>), Box<dyn std::error::Error>> {
    attempt_with(scripts, journal, manifest, &[]).await
}

async fn attempt_with(
    scripts: Vec<Script>,
    journal: Vec<ServerFrame>,
    manifest: &Manifest,
    followups: &[String],
) -> Result<(Followed, Vec<String>), Box<dyn std::error::Error>> {
    let (base, shared) = stand(Stand {
        scripts,
        journal,
        requests: vec![],
    })
    .await?;
    let token = tempfile::NamedTempFile::new()?;
    std::fs::write(token.path(), "token")?;
    let client = ApiClient::new(&base, token.path(), Duration::from_secs(5))?;
    let plan = Plan {
        manifest,
        task: "Fix it.",
        followups,
        route: "fake",
        template: None,
        profile: None,
        workspace_id: "w-1",
        margin: Duration::ZERO,
    };
    let followed = follow(&client, &plan).await;
    let requests = shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .requests
        .clone();
    Ok((followed, requests))
}

#[tokio::test]
async fn tk_u_006_a_run_is_followed_to_its_end() -> TestResult {
    let log = vec![
        event(1, Event::RunStarted { run_id: run_id()? }),
        usage(2, 5),
        event(3, Event::RunCompleted { run_id: run_id()? }),
    ];
    let (followed, requests) = attempt(
        vec![Script {
            frames: log.clone(),
            hang: true,
        }],
        log.clone(),
        &manifest(100, 60),
    )
    .await?;
    assert_eq!(
        (
            followed.driver_failed,
            followed.deadline_passed,
            followed.over_budget
        ),
        (false, false, false)
    );
    assert_eq!(
        (followed.streamed.clone(), followed.replayed.clone()),
        (log.clone(), log)
    );
    assert!(!requests.iter().any(|request| request.ends_with("/cancel")));
    Ok(())
}

#[tokio::test]
async fn tk_u_006_a_broken_stream_reopens_after_the_last_sequence() -> TestResult {
    let log = vec![
        event(1, Event::RunStarted { run_id: run_id()? }),
        usage(2, 1),
        event(3, Event::RunCompleted { run_id: run_id()? }),
    ];
    let scripts = vec![
        Script {
            frames: log[..2].to_vec(),
            hang: false,
        },
        Script {
            frames: log[2..].to_vec(),
            hang: true,
        },
    ];
    let (followed, requests) = attempt(scripts, log.clone(), &manifest(100, 60)).await?;
    assert_eq!(followed.streamed, log);
    let streams: Vec<_> = requests
        .iter()
        .filter(|request| request.contains("stream=1"))
        .collect();
    assert_eq!(streams.len(), 2);
    assert!(
        streams[0].contains("after=0") && streams[1].contains("after=2"),
        "{streams:?}"
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_006_budget_and_deadline_cancel_the_run() -> TestResult {
    let over = vec![
        event(1, Event::RunStarted { run_id: run_id()? }),
        usage(2, 500),
        event(3, Event::RunCancelled { run_id: run_id()? }),
    ];
    let (followed, requests) = attempt(
        vec![Script {
            frames: over.clone(),
            hang: true,
        }],
        over,
        &manifest(100, 60),
    )
    .await?;
    assert!(followed.over_budget && !followed.deadline_passed);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.ends_with("/runs/r-1/cancel"))
            .count(),
        1
    );
    // The driver's deadline is real time over real sockets: the task's minimum wall clock, one second.
    let silent = vec![event(1, Event::RunStarted { run_id: run_id()? })];
    let (followed, requests) = attempt(
        vec![Script {
            frames: silent.clone(),
            hang: true,
        }],
        silent,
        &manifest(100, 1),
    )
    .await?;
    assert!(followed.deadline_passed && !followed.driver_failed);
    assert!(
        requests
            .iter()
            .any(|request| request.ends_with("/runs/r-1/cancel"))
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_006_an_unreachable_stream_is_a_driver_failure() -> TestResult {
    let (followed, _) = attempt(vec![], vec![], &manifest(100, 60)).await?;
    assert!(followed.driver_failed);
    assert_eq!(
        followed.driver_detail.as_deref(),
        Some("open the event stream: the gateway answered 503")
    );
    assert_eq!(
        followed.conversation_id.map(|id| id.to_string()),
        Some(CONVERSATION.to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_006_an_undelivered_task_ends_the_attempt_at_once() -> TestResult {
    let notice = ServerFrame::Error {
        seq: None,
        code: tqa_types::ErrorCode::SandboxFailed,
        message: "The last message was not delivered: its sandbox failed.".into(),
        retryable: true,
        target: None,
    };
    let scripts = vec![Script {
        frames: vec![notice],
        hang: true,
    }];
    let started = std::time::Instant::now();
    let (followed, _) = attempt(scripts, vec![], &manifest(100, 60)).await?;
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the attempt must not wait for its deadline"
    );
    assert!(followed.driver_failed && !followed.deadline_passed);
    assert_eq!(
        followed.driver_detail.as_deref(),
        Some(
            "deliver the task: sandbox_failed: The last message was not delivered: its sandbox failed."
        )
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_006_follow_up_messages_are_sent_after_each_completed_run() -> TestResult {
    let second: RunId = serde_json::from_value(json!("r-2"))?;
    let first = vec![
        event(1, Event::RunStarted { run_id: run_id()? }),
        event(2, Event::RunCompleted { run_id: run_id()? }),
    ];
    let next = vec![
        event(
            3,
            Event::RunStarted {
                run_id: second.clone(),
            },
        ),
        event(4, Event::RunCompleted { run_id: second }),
    ];
    let journal: Vec<ServerFrame> = first.iter().chain(&next).cloned().collect();
    let scripts = vec![
        Script {
            frames: first,
            hang: true,
        },
        Script {
            frames: next,
            hang: true,
        },
    ];
    let followups = ["Now write the report.".to_owned()];
    let (followed, requests) =
        attempt_with(scripts, journal.clone(), &manifest(100, 60), &followups).await?;
    assert!(!followed.driver_failed && !followed.deadline_passed);
    assert_eq!(followed.streamed, journal);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.ends_with("/messages"))
            .count(),
        2
    );
    let streams: Vec<_> = requests
        .iter()
        .filter(|request| request.contains("stream=1"))
        .collect();
    assert!(
        streams.len() == 2 && streams[1].contains("after=2"),
        "{streams:?}"
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_006_a_failed_run_sends_no_follow_up() -> TestResult {
    let failed = vec![
        event(1, Event::RunStarted { run_id: run_id()? }),
        event(
            2,
            Event::RunFailed {
                run_id: run_id()?,
                reason: "r".into(),
                code: tqa_types::ErrorCode::Budget,
                provider_message: None,
            },
        ),
    ];
    let followups = ["never sent".to_owned()];
    let scripts = vec![Script {
        frames: failed.clone(),
        hang: true,
    }];
    let (followed, requests) =
        attempt_with(scripts, failed, &manifest(100, 60), &followups).await?;
    assert!(!followed.driver_failed);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.ends_with("/messages"))
            .count(),
        1
    );
    Ok(())
}
