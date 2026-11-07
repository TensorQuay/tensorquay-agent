use super::*;
use serde_json::json;
use tqa_types::RunId;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn frame(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-09-26T00:00:00Z".into(),
        event,
    }
}
fn run() -> Result<RunId, serde_json::Error> {
    serde_json::from_value(json!("r-1"))
}
fn tool_error(seq: u64, content: &str) -> Result<ServerFrame, serde_json::Error> {
    let call_id = serde_json::from_value(json!("c-1"))?;
    Ok(frame(
        seq,
        Event::ToolResult {
            call_id,
            status: ToolStatus::Error,
            content: content.into(),
            target: None,
            evidence: None,
        },
    ))
}
fn failed(code: ErrorCode) -> Result<ServerFrame, serde_json::Error> {
    Ok(frame(
        9,
        Event::RunFailed {
            run_id: run()?,
            reason: "r".into(),
            code,
            provider_message: None,
        },
    ))
}

struct Case {
    events: Vec<ServerFrame>,
    check: Option<CheckOutcome>,
    over_budget: bool,
    expected: (Class, Option<u64>),
}
fn case(
    events: Vec<ServerFrame>,
    check: Option<CheckOutcome>,
    class: Class,
    at: Option<u64>,
) -> Case {
    Case {
        events,
        check,
        over_budget: false,
        expected: (class, at),
    }
}
fn completed() -> Result<ServerFrame, serde_json::Error> {
    Ok(frame(9, Event::RunCompleted { run_id: run()? }))
}
fn cancelled() -> Result<ServerFrame, serde_json::Error> {
    Ok(frame(9, Event::RunCancelled { run_id: run()? }))
}
const PASSED: Option<CheckOutcome> = Some(CheckOutcome::Passed);

/// The harness rows of the design's classification table.
fn harness_rows() -> Result<Vec<Case>, serde_json::Error> {
    let tool_then_done =
        |content| Ok::<_, serde_json::Error>(vec![tool_error(3, content)?, completed()?]);
    Ok(vec![
        case(vec![], None, Class::HarnessTimeout, None),
        case(
            tool_then_done("io: pipe missing")?,
            PASSED,
            Class::HarnessTool,
            Some(3),
        ),
        case(
            tool_then_done("proxy_authentication: refused")?,
            PASSED,
            Class::HarnessTool,
            Some(3),
        ),
        case(
            vec![failed(ErrorCode::Internal)?],
            None,
            Class::HarnessRun,
            Some(9),
        ),
        case(
            vec![failed(ErrorCode::SandboxFailed)?],
            None,
            Class::HarnessRun,
            Some(9),
        ),
        case(
            vec![failed(ErrorCode::ProviderError)?],
            None,
            Class::HarnessProvider,
            Some(9),
        ),
        case(
            vec![completed()?],
            Some(CheckOutcome::Broken),
            Class::HarnessCheck,
            Some(9),
        ),
        case(vec![completed()?], None, Class::HarnessCheck, Some(9)),
        // Only our own budget cancel is the model's; any other cancel is unexplained.
        case(vec![cancelled()?], None, Class::HarnessRun, Some(9)),
    ])
}

/// The model and pass rows of the design's classification table.
fn model_rows() -> Result<Vec<Case>, serde_json::Error> {
    Ok(vec![
        // A tool error caused by the model's request is feedback, not a harness failure.
        case(
            vec![tool_error(3, "not_found: no such file")?, completed()?],
            PASSED,
            Class::Pass,
            Some(9),
        ),
        case(
            vec![completed()?],
            Some(CheckOutcome::Tampered),
            Class::ModelTampered,
            Some(9),
        ),
        case(
            vec![completed()?],
            Some(CheckOutcome::Failed),
            Class::ModelWrongWork,
            Some(9),
        ),
        case(
            vec![failed(ErrorCode::Budget)?],
            None,
            Class::ModelBudget,
            Some(9),
        ),
        case(
            vec![failed(ErrorCode::FallbackParse)?],
            None,
            Class::ModelUnreadable,
            Some(9),
        ),
        case(
            vec![failed(ErrorCode::NoProgress)?],
            None,
            Class::ModelStuck,
            Some(9),
        ),
        // A decision engine is part of the agent under test: its stop is the agent's own failure (ADR-0013).
        case(
            vec![failed(ErrorCode::DecisionStop)?],
            None,
            Class::ModelBudget,
            Some(9),
        ),
        Case {
            over_budget: true,
            ..case(vec![cancelled()?], None, Class::ModelBudget, Some(9))
        },
        // The same tool call failed three times, each refused by our own tool's argument check: its own class, for the
        // hand review, not the model's budget (the 13h Gate 5 demo, 2 Oct 2026).
        case(
            vec![
                tool_error(
                    6,
                    "invalid_args: /cursor: not a cursor from an earlier read_file",
                )?,
                tool_error(
                    7,
                    "invalid_args: /cursor: not a cursor from an earlier read_file",
                )?,
                tool_error(
                    8,
                    "invalid_args: /cursor: not a cursor from an earlier read_file",
                )?,
                failed(ErrorCode::RepeatedFailure)?,
            ],
            None,
            Class::ModelToolArgs,
            Some(9),
        ),
        // Three failures of the model's own making (a file that isn't there) stay the model's: it is stuck, not out of
        // budget (c14b-rc's E2E-WAVE-002 miss, 7 Oct 2026: the same failing test run three times).
        case(
            vec![
                tool_error(6, "not_found: src/missing.rs")?,
                tool_error(7, "not_found: src/missing.rs")?,
                tool_error(8, "not_found: src/missing.rs")?,
                failed(ErrorCode::RepeatedFailure)?,
            ],
            None,
            Class::ModelStuck,
            Some(9),
        ),
        case(vec![completed()?], PASSED, Class::Pass, Some(9)),
    ])
}

#[test]
fn tk_u_004_each_rule_of_the_table_decides_its_class() -> TestResult {
    for Case {
        events,
        check,
        over_budget,
        expected,
    } in harness_rows()?.into_iter().chain(model_rows()?)
    {
        let observed = Observation {
            driver_failed: false,
            integrity: Ok(()),
            over_budget,
            deadline_passed: false,
            events: &events,
            check,
        };
        let verdict = classify(&observed);
        let found = (verdict.class, verdict.evidence_seq);
        assert_eq!(
            found,
            (expected.0, expected.1.map(Seq::from)),
            "{events:?} {check:?}"
        );
    }
    Ok(())
}

#[test]
fn tk_u_004_driver_and_integrity_faults_win_over_everything() -> TestResult {
    let events = [completed()?];
    let base = Observation {
        driver_failed: true,
        integrity: Ok(()),
        over_budget: false,
        deadline_passed: false,
        events: &events,
        check: Some(CheckOutcome::Passed),
    };
    assert_eq!(classify(&base).class, Class::HarnessDriver);
    let lost = Observation {
        driver_failed: false,
        integrity: Err(IntegrityFault::Duplicate { seq: 4.into() }),
        ..base
    };
    assert_eq!(
        classify(&lost),
        Verdict {
            class: Class::HarnessLostEvent,
            evidence_seq: Some(4.into())
        }
    );
    // Our deadline passed and we cancelled: the cancel we caused must not read as an unexplained stop.
    let late = [cancelled()?];
    let timed_out = Observation {
        driver_failed: false,
        deadline_passed: true,
        events: &late,
        check: None,
        ..base
    };
    assert_eq!(classify(&timed_out).class, Class::HarnessTimeout);
    let model = [
        Class::Pass,
        Class::ModelTampered,
        Class::ModelWrongWork,
        Class::ModelToolArgs,
        Class::ModelStuck,
        Class::ModelUnreadable,
        Class::ModelBudget,
    ];
    assert!(model.iter().all(|class| !class.is_harness()));
    // The stuck and unreadable stops are the model's, under names of their own (7 Oct 2026, backlog #16).
    assert_eq!(
        serde_json::to_value(Class::ModelStuck)?,
        json!("model.stuck")
    );
    assert_eq!(
        serde_json::to_value(Class::ModelUnreadable)?,
        json!("model.unreadable")
    );
    assert!(Class::HarnessTaskDesign.is_harness() && Class::HarnessProvider.is_harness());
    assert_eq!(
        serde_json::to_value(Class::HarnessLostEvent)?,
        json!("harness.lost_event")
    );
    Ok(())
}

#[test]
fn tk_u_004_a_refused_call_is_kept_for_its_hand_review() -> TestResult {
    let call_id: tqa_types::ToolCallId = serde_json::from_value(json!("c-1"))?;
    let events = vec![
        frame(
            5,
            Event::ToolCall {
                call_id,
                name: "read_file".into(),
                arguments: json!({"cursor": "", "path": "ledger/part-05.txt"}),
            },
        ),
        tool_error(
            6,
            "invalid_args: /cursor: not a cursor from an earlier read_file",
        )?,
        failed(ErrorCode::RepeatedFailure)?,
    ];
    assert_eq!(
        refused_call(&events).as_deref(),
        Some(
            "read_file {\"cursor\":\"\",\"path\":\"ledger/part-05.txt\"} refused: invalid_args: /cursor: not a \
             cursor from an earlier read_file"
        )
    );
    assert!(!Class::ModelToolArgs.is_harness(), "it never blocks a gate");
    assert_eq!(refused_call(&events[2..]), None);
    Ok(())
}
