use super::*;
use crate::e2e::classify::Class;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Spoils one field and names it.
type Spoil = (fn(&mut Environment), &'static str);

fn environment() -> Environment {
    let digest = format!("sha256:{}", "b".repeat(64));
    Environment {
        commit: "a".repeat(40),
        dirty: false,
        images: [
            ("runtime".into(), digest.clone()),
            ("gateway".into(), digest),
        ]
        .into(),
        route: "fake".into(),
        native_tools: true,
        fallback_prompt_version: tqa_prompt::FALLBACK_PROMPT_VERSION,
        model: "fake-model".into(),
        host: Host {
            cpu: "cpu".into(),
            cores: 24,
            memory_bytes: 1,
            kernel: "7.0".into(),
            docker: "28".into(),
            gpu: None,
        },
        driver_version: "0.1.0".into(),
        flags: vec!["--token-file".into(), "/run/secrets/e2e-token".into()],
    }
}

fn frame(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-09-26T00:00:00Z".into(),
        event,
    }
}

#[test]
fn tk_u_005_an_incomplete_environment_names_its_first_missing_field() {
    let cases: [Spoil; 7] = [
        (|env| env.commit = "abc".into(), "commit"),
        (|env| env.commit = "A".repeat(40), "commit"),
        (
            |env| {
                env.images.remove("gateway");
            },
            "images",
        ),
        (
            |env| {
                env.images.insert("web".into(), "latest".into());
            },
            "images",
        ),
        (|env| env.route.clear(), "route"),
        (
            |env| env.fallback_prompt_version = 0,
            "fallback_prompt_version",
        ),
        (|env| env.host.docker.clear(), "host.docker"),
    ];
    assert_eq!(environment().validate(), Ok(()));
    for (spoil, field) in cases {
        let mut env = environment();
        spoil(&mut env);
        assert_eq!(env.validate(), Err(Incomplete(field)));
    }
}

/// An attempt as `tk_u_005` writes it.
fn sample_attempt(tally: Tally) -> Attempt {
    Attempt {
        task_id: "py-fix".into(),
        attempt: 1,
        rerun_of: None,
        conversation_id: None,
        wall_ms: 1234,
        tally,
        check: Some(CheckOutcome::Failed),
        detail: None,
        verdict: Verdict {
            class: Class::ModelWrongWork,
            evidence_seq: Some(3.into()),
        },
        workspace_id: Some("0192a7d4-0000-7000-8000-00000000000a".into()),
        arm: Arm::Model,
        round: 2,
        pair_position: 1,
        decisions: DecisionTally::default(),
    }
}

#[test]
fn tk_u_005_run_and_attempt_files_have_the_schema_and_no_token() -> TestResult {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("run");
    let writer = ResultWriter::create(&directory, &environment())?;
    assert!(
        ResultWriter::create(&directory, &environment()).is_err(),
        "a run is never overwritten"
    );
    let mut spoiled = environment();
    spoiled.commit.clear();
    assert!(ResultWriter::create(&root.path().join("other"), &spoiled).is_err());
    let usage = Event::Usage {
        prompt_tokens: 100,
        completion_tokens: 7,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: false,
        estimated_input: None,
        estimated_images: None,
        estimated_route: None,
    };
    let events = [
        frame(1, usage.clone()),
        frame(2, usage),
        frame(
            3,
            Event::StepCompleted {
                step: 1,
                duration_ms: 5,
            },
        ),
    ];
    let tally = Tally::from_events(&events);
    assert_eq!(
        tally,
        Tally {
            steps: 1,
            prompt_tokens: 200,
            completion_tokens: 14,
            compactions: 0,
            provider_retries: 0
        }
    );
    writer.append(&sample_attempt(tally))?;
    let run: Value = serde_json::from_str(&std::fs::read_to_string(directory.join("run.json"))?)?;
    assert_eq!(run["schema_version"], json!(SCHEMA_VERSION));
    assert_eq!(run["environment"]["commit"], json!("a".repeat(40)));
    // The retry sentence a model-fallback run had (ADR-0007): the build's own instruction version.
    assert_eq!(run["environment"]["fallback_prompt_version"], json!(5));
    let lines = std::fs::read_to_string(directory.join("attempts.jsonl"))?;
    let attempt: Value = serde_json::from_str(lines.trim_end())?;
    assert_eq!(attempt["class"], json!("model.wrong_work"));
    // TK-U-027: which workspace the attempt used, so a leftover can be found.
    assert_eq!(
        attempt["workspace_id"],
        json!("0192a7d4-0000-7000-8000-00000000000a")
    );
    assert_eq!(
        (
            attempt["evidence_seq"].clone(),
            attempt["prompt_tokens"].clone()
        ),
        (json!(3), json!(200))
    );
    assert_eq!(attempt["check"], json!("failed"));
    assert_eq!(attempt["arm"], json!("model"));
    assert_eq!(attempt["pair_position"], json!(1));
    // E2E-DECISION-008 pairs the arms by task and round.
    assert_eq!(attempt["round"], json!(2));
    let written = format!("{run}{lines}");
    assert!(
        !written.contains("tqa_pat_"),
        "a result never carries a token"
    );
    Ok(())
}
