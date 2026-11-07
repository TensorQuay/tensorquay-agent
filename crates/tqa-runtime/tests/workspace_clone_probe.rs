//! RT-I-015: test-only Linux executable for the real policy-proxy clone fixture.
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio_util::sync::CancellationToken;
use tqa_core::{EventContext, EventSink, SinkError};
use tqa_runtime::testing::WorkspaceInitializer;
use tqa_testkit::{FakeClock, FakeSleeper};
use tqa_tools::{GitCloner, ProxyEndpoint};
use tqa_types::{Event, Seq, ServerFrame, WorkspaceClone};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    url: String,
    proxy: String,
}
#[derive(Default)]
struct Sink(AtomicU64);
#[async_trait]
impl EventSink for Sink {
    async fn emit(&self, _event: Event, _context: EventContext) -> Result<Seq, SinkError> {
        Ok(Seq::from(self.0.fetch_add(1, Ordering::SeqCst) + 1))
    }
}

#[tokio::test]
#[ignore = "test-only probe; execute inside the owned sandbox with TQA_CLONE_FIXTURE"]
async fn workspace_clone_probe() -> TestResult {
    let root = fs::canonicalize(std::env::var("TQA_CLONE_FIXTURE")?)?;
    let bytes = fs::read(root.join("input.json"))?;
    assert!(bytes.len() <= 8192, "fixture input is bounded");
    let input: Input = serde_json::from_slice(&bytes)?;
    let mut results = BTreeMap::new();
    for (name, reference) in [
        ("main", None),
        ("branch", Some("feature/example")),
        ("tag", Some("v1")),
        ("denied", None),
        ("preserved", None),
    ] {
        results.insert(name, exercise(&root, &input, name, reference).await?);
    }
    fs::write(root.join("results.json"), serde_json::to_vec(&results)?)?;
    Ok(())
}

async fn exercise(
    root: &Path,
    input: &Input,
    name: &str,
    reference: Option<&str>,
) -> TestResult<String> {
    let workspace = root.join(name);
    let state_dir = root.join(format!("{name}-state"));
    fs::create_dir(&workspace)?;
    fs::create_dir(&state_dir)?;
    if name == "preserved" {
        fs::write(workspace.join("README.md"), "canary\n")?;
    }
    let repository = WorkspaceClone {
        url: if matches!(name, "denied" | "preserved") {
            "https://denied.invalid/repo.git".into()
        } else {
            input.url.clone()
        },
        r#ref: reference.map(str::to_owned),
    };
    let sleeper = Arc::new(FakeSleeper::new(FakeClock::new()));
    let initializer = WorkspaceInitializer {
        workspace: workspace.clone(),
        state_dir,
        proxy: Some(ProxyEndpoint::new(&input.proxy)?),
        cloner: Arc::new(
            GitCloner::new(sleeper.clone()).with_ca_bundle(root.join("fixture-ca.pem")),
        ),
        sleeper,
    };
    let sink = Sink::default();
    let mut history = vec![];
    let failure = initializer
        .initialize(
            Some(&repository),
            &mut history,
            &sink,
            &CancellationToken::new(),
        )
        .await?;
    if let Some(failure) = failure {
        assert_eq!(
            name, "denied",
            "allowed checkout failed: {}",
            failure.reason
        );
        assert!(!workspace.join("README.md").exists());
        return Ok(serde_json::to_value(failure.code)?
            .as_str()
            .ok_or("error code")?
            .into());
    }
    let expected_status = assert_checkout(&workspace, name, &history)?;
    let count = sink.0.load(Ordering::SeqCst);
    assert!(
        initializer
            .initialize(
                Some(&repository),
                &mut history,
                &sink,
                &CancellationToken::new()
            )
            .await?
            .is_none()
    );
    assert_eq!(
        sink.0.load(Ordering::SeqCst),
        count,
        "initialization repeated"
    );
    Ok(expected_status.into())
}

fn assert_checkout(
    workspace: &Path,
    name: &str,
    history: &[ServerFrame],
) -> TestResult<&'static str> {
    let expected = match name {
        "main" => "main\n",
        "branch" => "branch\n",
        "tag" => "seed\n",
        "preserved" => "canary\n",
        _ => return Err("denied clone succeeded".into()),
    };
    assert_eq!(fs::read_to_string(workspace.join("README.md"))?, expected);
    let expected_status = if name == "preserved" {
        "skipped"
    } else {
        "completed"
    };
    let event = serde_json::to_value(history.last().ok_or("clone status")?)?;
    assert_eq!(event["event"]["status"], expected_status);
    if name != "preserved" {
        assert!(
            !workspace.join(".git/shallow").exists(),
            "clone must preserve full history"
        );
    }
    Ok(expected_status)
}
