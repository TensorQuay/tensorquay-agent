//! TOOL-U-056: the lead's `done` runs the administrator's acceptance check (ADR-0034 point 10, GW-I-198) with real
//! processes in the workspace, through the shell tool. It sends the exit and the end of the output with the summary; a
//! check past its time limit is reported stopped, its process group gone; a Stop ends it and sends nothing. Without a
//! check (unset, or an older gateway without the route) `done` sends its summary alone. A refused check comes back as
//! the tool's result: how it ended and its last output, so the lead can fix the work and call `done` again.
#[path = "support/context.rs"]
mod support;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tqa_tools::Tool;
use tqa_tools::builtin::Shell;
use tqa_tools::builtin::team::{
    ACCEPTANCE_TIMEOUT, AcceptanceShell, ReadAnswer, TeamClient, TeamFailure, TeamRole,
    tools_with_check,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A gateway with a set check (or none), answering `done` as told and recording what it was sent.
#[derive(Default)]
struct Gateway {
    check: Option<Result<Option<String>, TeamFailure>>,
    refuse: Option<TeamFailure>,
    sent: Mutex<Vec<Value>>,
}

fn unused() -> TeamFailure {
    TeamFailure::new("unused", "unused")
}

#[async_trait]
impl TeamClient for Gateway {
    async fn start(
        &self,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        Err(unused())
    }
    async fn post(
        &self,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        Err(unused())
    }
    async fn take_files(
        &self,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        Err(unused())
    }
    async fn done(
        &self,
        body: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        self.sent.lock().map_err(|_| unused())?.push(body.clone());
        self.refuse
            .clone()
            .map_or_else(|| Ok(json!({"team": {"state": "stopping"}})), Err)
    }
    async fn stop_member(
        &self,
        _: &str,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        Err(unused())
    }
    async fn transcript(
        &self,
        _: &str,
        _: (Option<u64>, Option<u32>),
        _: &str,
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        Err(unused())
    }
    async fn read(
        &self,
        _: (Option<u32>, Option<u32>),
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<ReadAnswer, TeamFailure> {
        Err(unused())
    }
    async fn acceptance(
        &self,
        _: &str,
        _: &CancellationToken,
    ) -> Result<Option<String>, TeamFailure> {
        self.check.clone().unwrap_or(Ok(None))
    }
}

fn set(command: &str) -> Gateway {
    Gateway {
        check: Some(Ok(Some(command.to_owned()))),
        ..Gateway::default()
    }
}

fn done(gateway: &Arc<Gateway>, check: Option<AcceptanceShell>) -> Result<Arc<dyn Tool>, String> {
    tools_with_check(gateway.clone(), TeamRole::Lead, check)
        .into_iter()
        .find(|tool| tool.spec().name.as_str() == "done")
        .ok_or_else(|| "no done".to_owned())
}

fn shell() -> AcceptanceShell {
    AcceptanceShell::new(Arc::new(Shell::new()))
}

fn sent(gateway: &Gateway) -> Result<Vec<Value>, String> {
    Ok(gateway.sent.lock().map_err(|_| "lock")?.clone())
}

const SUMMARY: &str = "Q3 table in out/q3.md.";

#[tokio::test]
async fn tool_u_056_done_sends_the_checks_exit_and_output_with_the_summary() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    // It runs in the workspace root: the file it reads is there.
    std::fs::write(
        fixture.workspace().path().join("result.txt"),
        "2 tests failed\n",
    )?;
    let gateway = Arc::new(set("cat result.txt; exit 3"));
    done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await?;
    assert_eq!(
        sent(&gateway)?,
        [
            json!({"summary": SUMMARY, "check": {"command": "cat result.txt; exit 3", "exit_code": 3,
            "timed_out": false, "output": "2 tests failed\n"}})
        ]
    );
    // A passing check: exit 0.
    let gateway = Arc::new(set("true"));
    let answer = done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await?;
    assert_eq!(
        answer.text,
        "Your helpers are stopping, and the end note follows. Finish your answer to the person."
    );
    assert_eq!(sent(&gateway)?[0]["check"]["exit_code"], 0);
    // 10 KiB of output: the last 4 KiB go.
    let gateway = Arc::new(set(
        "head -c 6144 /dev/zero | tr '\\0' a; head -c 4096 /dev/zero | tr '\\0' b",
    ));
    done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await?;
    assert_eq!(sent(&gateway)?[0]["check"]["output"], "b".repeat(4096));
    Ok(())
}

#[tokio::test]
async fn tool_u_056_a_check_past_its_limit_is_stopped_with_its_process_group() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    assert_eq!(
        ACCEPTANCE_TIMEOUT,
        Duration::from_mins(10),
        "the shell's own 30 s is far too short"
    );
    let gateway = Arc::new(set("sleep 60 & echo $! > child; wait"));
    let started = Instant::now();
    done(&gateway, Some(shell().with_timeout(Duration::from_secs(1))))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await?;
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    let check = &sent(&gateway)?[0]["check"];
    assert_eq!(
        (&check["timed_out"], &check["exit_code"]),
        (&json!(true), &Value::Null)
    );
    // Its background child was stopped with it (given a moment to be reaped).
    let child = std::fs::read_to_string(fixture.workspace().path().join("child"))?;
    let proc = std::path::PathBuf::from(format!("/proc/{}", child.trim()));
    let gone = Instant::now();
    while proc.exists() && gone.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!proc.exists(), "the check's background child still runs");
    Ok(())
}

#[tokio::test]
async fn tool_u_056_a_stop_ends_the_check_and_sends_nothing() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    let gateway = Arc::new(set("sleep 60"));
    let stop = cx.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        stop.cancel();
    });
    let started = Instant::now();
    let error = done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await
        .err()
        .ok_or("finished")?;
    assert!(
        matches!(error, tqa_tools::ToolError::Cancelled),
        "{error:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(sent(&gateway)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn tool_u_056_without_a_check_done_sends_its_summary_alone() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    let older = TeamFailure::new("not_found", "record not found");
    let ended = TeamFailure::new("team_ended", "the agent team has ended");
    for gateway in [
        Gateway::default(),
        Gateway {
            check: Some(Err(older)),
            ..Gateway::default()
        },
        // done answers this one itself, with what the lead hadn't read.
        Gateway {
            check: Some(Err(ended)),
            ..Gateway::default()
        },
    ] {
        let gateway = Arc::new(gateway);
        done(&gateway, Some(shell()))?
            .call(json!({"summary": SUMMARY}), &cx)
            .await?;
        assert_eq!(sent(&gateway)?, [json!({"summary": SUMMARY})]);
    }
    // Asking failed otherwise: refused as the gateway said, nothing sent.
    let gateway = Arc::new(Gateway {
        check: Some(Err(TeamFailure::new(
            "store_unavailable",
            "the store is unavailable",
        ))),
        ..Gateway::default()
    });
    let error = done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await
        .err()
        .ok_or("done")?;
    assert_eq!(
        error.model_message(),
        "store_unavailable: the store is unavailable"
    );
    assert!(sent(&gateway)?.is_empty());
    // A check set for a task with no shell tool: refused in words, nothing sent.
    let gateway = Arc::new(set("true"));
    let error = done(&gateway, None)?
        .call(json!({"summary": SUMMARY}), &cx)
        .await
        .err()
        .ok_or("done")?;
    assert!(
        error
            .model_message()
            .starts_with("acceptance_failed: the acceptance check can't run"),
        "{}",
        error.model_message()
    );
    assert!(sent(&gateway)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn tool_u_056_a_refused_check_shows_how_it_ended_and_its_last_output() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    let refused = |details: Value| {
        let mut failure = TeamFailure::new("acceptance_failed", "the acceptance check failed");
        failure.details = Some(details);
        Arc::new(Gateway {
            check: Some(Ok(Some("false".into()))),
            refuse: Some(failure),
            ..Gateway::default()
        })
    };
    let gateway =
        refused(json!({"exit_code": 1, "timed_out": false, "output": "[stderr] 2 tests failed\n"}));
    let answer = done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await?;
    assert_eq!(
        answer.text,
        "acceptance_failed: the acceptance check failed (exit 1), so the team goes on. Fix what it found, then call \
         done again. Its last output:\n[stderr] 2 tests failed\n"
    );
    assert_eq!(answer.exit_code, Some(1));
    let gateway = refused(json!({"exit_code": null, "timed_out": true, "output": ""}));
    let answer = done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await?;
    assert!(
        answer.text.contains("(stopped after 10 minutes)")
            && answer.text.ends_with("It printed nothing."),
        "{}",
        answer.text
    );
    // Without details (an older runtime's missing report, a changed check), the gateway's words as a refusal.
    let gateway = Arc::new(Gateway {
        refuse: Some(TeamFailure::new(
            "acceptance_failed",
            "the acceptance check changed while it ran; call done again",
        )),
        ..Gateway::default()
    });
    let error = done(&gateway, Some(shell()))?
        .call(json!({"summary": SUMMARY}), &cx)
        .await
        .err()
        .ok_or("done")?;
    assert_eq!(
        error.model_message(),
        "acceptance_failed: the acceptance check changed while it ran; call done again"
    );
    Ok(())
}
