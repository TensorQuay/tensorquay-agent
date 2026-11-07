//! RT-I-013: the journal, advertised tools and prompt agree on the environment.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, ErrorCode, ServerFrame};

fn configured(
    template: &str,
    capabilities: &[&str],
) -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["environment"] = json!({
        "template": template, "digest": format!("sha256:{}", "a".repeat(64)),
        "capabilities": capabilities,
    });
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

fn events(f: &Fixture) -> Result<Vec<Value>, serde_json::Error> {
    f.gateway
        .stored()
        .iter()
        .map(serde_json::to_value)
        .collect()
}

#[tokio::test]
async fn rt_i_013_workstation_change_precedes_the_first_run_and_reaches_the_model() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    f.gateway.set_session(configured(
        "workstation",
        &["shell", "python", "node", "build", "browser"],
    )?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    let frames = events(&f)?;
    assert_eq!(frames[0]["seq"], 1);
    assert_eq!(frames[0]["event"]["type"], "environment.changed");
    assert_eq!(frames[0]["event"]["template"], "workstation");
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["event"]["type"] == "environment.changed")
            .count(),
        1
    );
    let requests = f.gateway.model_requests();
    let system = requests[0].body["messages"][0]["content"]
        .as_str()
        .ok_or("system prompt")?;
    assert!(system.contains("Environment: workstation"), "{system}");
    assert!(
        system.contains("Capabilities: shell, python, node, build, browser"),
        "{system}"
    );
    // A notice before the first user message is one of the leading system messages, which go as one (LLM-U-033).
    assert!(
        system.ends_with(
            "\n\nEnvironment changed to workstation. Capabilities: shell, python, node, build, browser."
        ),
        "{system}"
    );
    let messages = requests[0].body["messages"].as_array().ok_or("messages")?;
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["role"] == "system")
            .count(),
        1
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_013_each_capability_advertises_only_its_allowed_builtin_tools() -> TestResult {
    for capabilities in [
        &[][..],
        &["shell"],
        &["python"],
        &["node"],
        &["build"],
        &["browser"],
    ] {
        let f = Fixture::new(vec![answer()]).await?;
        f.gateway.set_session(configured("custom", capabilities)?)?;
        f.gateway.set_bound(true);
        let (shutdown, task) = f.launch()?;
        f.gateway.wait_for_registrations(1).await?;
        f.gateway.send(user()).await?;
        f.wait_event("run.completed").await?;
        let requests = f.gateway.model_requests();
        let tools = requests[0].body["tools"]
            .as_array()
            .ok_or("tool definitions")?;
        let mut actual: Vec<_> = tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        actual.sort_unstable();
        let mut expected = vec![
            "edit_file",
            "glob",
            "grep",
            "read_file",
            "read_notes",
            "write_file",
            "write_notes",
        ];
        if capabilities.contains(&"shell") {
            expected.push("shell");
        }
        expected.sort_unstable();
        assert_eq!(actual, expected, "capabilities: {capabilities:?}");
        // A profile's tool allow-list may name every advertised tool (ADR-0012).
        assert!(
            actual
                .iter()
                .all(|name| tqa_tools::builtin::BUILTIN_TOOL_NAMES.contains(name)),
            "{actual:?}"
        );
        stop(shutdown, task).await?;
        f.gateway.shutdown().await?;
    }
    Ok(())
}

fn event_count(f: &Fixture, name: &str) -> usize {
    f.gateway
        .stored()
        .iter()
        .filter(|frame| {
            matches!(frame,
                ServerFrame::Event { event, .. } if event_name(event) == name
            )
        })
        .count()
}

async fn action_round_trip(f: &Fixture) -> TestResult {
    let errors = || {
        f.gateway
            .frames()
            .iter()
            .filter(|frame| {
                matches!(
                    frame,
                    ServerFrame::Error {
                        code: ErrorCode::InvalidFrame,
                        ..
                    }
                )
            })
            .count()
    };
    let before = errors();
    f.gateway
        .send(ClientFrame::SessionInit {
            conversation_id: "request".parse()?,
            last_seq: tqa_types::Seq::ZERO,
            protocol: 1,
        })
        .await?;
    wait_for(|| errors() > before).await
}

async fn redial(f: &Fixture) -> TestResult {
    f.gateway.disconnect();
    let delay = Duration::from_millis(125);
    wait_for(|| f.sleeper.pending_durations().contains(&delay)).await?;
    f.clock.advance(delay)?;
    Ok(())
}

type RestartCase = (ClientFrame, bool, &'static str);

fn restart_snapshots() -> Result<Vec<RestartCase>, Box<dyn std::error::Error>> {
    let workstation = configured(
        "workstation",
        &["shell", "python", "node", "build", "browser"],
    )?;
    let mut repinned = serde_json::to_value(&workstation)?;
    repinned["environment"]["digest"] = json!(format!("sha256:{}", "b".repeat(64)));
    repinned["environment"]["capabilities"] =
        json!(["browser", "build", "node", "python", "shell"]);
    let repinned = ClientFrame::parse(&serde_json::to_vec(&repinned)?)?;
    Ok(vec![
        (session()?, false, "minimal"),
        (workstation, true, "workstation"),
        (repinned, false, "workstation"),
        (
            configured("custom", &["shell", "python", "node", "build", "browser"])?,
            true,
            "custom",
        ),
        (configured("custom", &["shell"])?, true, "custom"),
        (session()?, true, "minimal"),
        (session()?, false, "minimal"),
        (configured("minimal", &["shell"])?, false, "minimal"),
    ])
}

#[tokio::test]
async fn rt_i_013_legacy_gateway_and_restarts_compare_template_and_capabilities() -> TestResult {
    let snapshots = restart_snapshots()?;
    let f = Fixture::new(vec![answer(); snapshots.len()]).await?;
    f.gateway.set_bound(true);
    let mut older = serde_json::to_value(session()?)?;
    older
        .as_object_mut()
        .ok_or("session object")?
        .remove("environment");
    f.gateway.first_frame(serde_json::to_string(&older)?)?;
    for (index, (snapshot, changed, template)) in snapshots.into_iter().enumerate() {
        let before = f.gateway.stored();
        let previous_changes = event_count(&f, "environment.changed");
        f.gateway.set_session(snapshot)?;
        let (shutdown, task) = f.launch()?;
        f.gateway.wait_for_registrations(index + 1).await?;
        action_round_trip(&f).await?;
        f.gateway.send(user()).await?;
        wait_for(|| event_count(&f, "run.completed") == index + 1).await?;
        let frames = events(&f)?;
        assert_eq!(&f.gateway.stored()[..before.len()], before);
        assert_eq!(
            event_count(&f, "environment.changed"),
            previous_changes + usize::from(changed)
        );
        assert_eq!(
            frames[before.len()]["event"]["type"],
            if changed {
                "environment.changed"
            } else {
                "message.user"
            }
        );
        for (offset, frame) in frames.iter().enumerate() {
            assert_eq!(frame["seq"], offset + 1);
        }
        let requests = f.gateway.model_requests();
        let system = requests[index].body["messages"][0]["content"]
            .as_str()
            .ok_or("system")?;
        assert!(
            system.contains(&format!("Environment: {template}")),
            "{system}"
        );
        if index == 2 {
            assert_eq!(
                requests[1].body["messages"][0],
                requests[2].body["messages"][0]
            );
        }
        stop(shutdown, task).await?;
    }
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_013_reconnect_and_unacknowledged_wal_tail_do_not_duplicate_changes() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    f.gateway
        .set_session(configured("workstation", &["shell", "browser"])?)?;
    f.gateway.set_bound(true);
    f.gateway.set_persist(false);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.wait_event("environment.changed").await?;
    action_round_trip(&f).await?;
    assert!(f.gateway.stored().is_empty());
    stop(shutdown, task).await?;
    let (wal, recovered) = tqa_context::Wal::open(&f.directory.path().join("state"))?;
    assert_eq!(recovered.records.len(), 1);
    drop(wal);
    f.gateway.set_persist(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(2).await?;
    action_round_trip(&f).await?;
    assert_eq!(f.gateway.stored(), vec![recovered.records[0].1.clone()]);
    redial(&f).await?;
    f.gateway.wait_for_registrations(3).await?;
    action_round_trip(&f).await?;
    assert_eq!(event_count(&f, "environment.changed"), 1);
    assert_eq!(f.gateway.stored().len(), 1);
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_013_reconnect_freezes_the_full_environment_including_digest() -> TestResult {
    for field in ["template", "digest", "capabilities"] {
        let f = Fixture::new(vec![]).await?;
        let initial = configured("workstation", &["shell", "browser"])?;
        f.gateway.set_session(initial.clone())?;
        f.gateway.set_bound(true);
        let (_, task) = f.launch()?;
        f.gateway.wait_for_registrations(1).await?;
        action_round_trip(&f).await?;
        let mut next = serde_json::to_value(initial)?;
        next["environment"][field] = match field {
            "template" => json!("custom"),
            "capabilities" => json!(["shell"]),
            _ => json!(format!("sha256:{}", "b".repeat(64))),
        };
        f.gateway
            .set_session(ClientFrame::parse(&serde_json::to_vec(&next)?)?)?;
        redial(&f).await?;
        assert_eq!(
            tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task)
                .await??,
            Err(tqa_runtime::RuntimeError::SnapshotChanged("environment"))
        );
        assert_eq!(event_count(&f, "environment.changed"), 1);
        f.gateway.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_013_environment_change_is_journalled_before_run_resumed() -> TestResult {
    let history = [
        json!({"type":"message.user","text":"resume request","attachments":[]}),
        json!({"type":"run.started","run_id":"r-1"}),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, event)| {
        serde_json::from_value(json!({"type":"event","seq":index+1,"run_id":null,
            "step":null,"ts":"2000-01-01T00:00:00Z","event":event}))
    })
    .collect::<Result<Vec<ServerFrame>, _>>()?;
    let f = Fixture::with_history(vec![answer()], history.clone()).await?;
    f.gateway
        .set_session(configured("workstation", &["shell", "python"])?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.wait_event("run.completed").await?;
    assert_eq!(&f.gateway.stored()[..history.len()], history);
    let frames = events(&f)?;
    assert_eq!(frames[2]["event"]["type"], "environment.changed");
    assert_eq!(frames[2]["seq"], 3);
    assert_eq!(frames[3]["event"]["type"], "run.resumed");
    assert_eq!(event_count(&f, "environment.changed"), 1);
    let messages = &f.gateway.model_requests()[0].body["messages"];
    assert!(
        messages
            .as_array()
            .ok_or("messages")?
            .iter()
            .any(|message| {
                message["role"] == "user"
                    && message["content"]
                        == "[System notice] Environment changed to workstation. Capabilities: shell, python."
            })
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_013_an_unadvertised_shell_call_cannot_execute() -> TestResult {
    let f = Fixture::new(vec![
        Script::new(vec![Scripted::ToolCall {
            name: "shell".into(),
            arguments_json: json!({"command":"printf unexpected > forbidden-shell-output"})
                .to_string(),
            split_into: 1,
        }]),
        answer(),
    ])
    .await?;
    f.gateway.set_session(configured("custom", &["browser"])?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    assert!(
        !f.directory
            .path()
            .join("workspace/forbidden-shell-output")
            .exists()
    );
    assert!(events(&f)?.iter().any(
        |frame| frame["event"]["type"] == "tool.result" && frame["event"]["status"] == "error"
    ));
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

/// `configured` with the image `digest` and, when given, the template `version` it was moved to (revision 8).
fn versioned(
    digest: char,
    version: Option<u64>,
) -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(configured("workstation", &["shell"])?)?;
    value["environment"]["digest"] = json!(format!("sha256:{}", digest.to_string().repeat(64)));
    if let Some(version) = version {
        value["environment"]["version"] = json!(version);
    }
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

// TEST: RT-I-029
#[tokio::test]
async fn rt_i_029_an_image_only_update_is_announced_once_by_its_version() -> TestResult {
    // (session, the version announced, or none when nothing is)
    let cases: Vec<(ClientFrame, Option<Option<u64>>)> = vec![
        (versioned('a', None)?, Some(None)),
        (versioned('b', Some(2))?, Some(Some(2))),
        (versioned('b', Some(2))?, None),
        (versioned('c', Some(3))?, Some(Some(3))),
        (versioned('c', None)?, None),
    ];
    let f = Fixture::new(vec![answer(); cases.len()]).await?;
    f.gateway.set_bound(true);
    for (index, (snapshot, announced)) in cases.into_iter().enumerate() {
        let before = event_count(&f, "environment.changed");
        f.gateway.set_session(snapshot)?;
        let (shutdown, task) = f.launch()?;
        f.gateway.wait_for_registrations(index + 1).await?;
        action_round_trip(&f).await?;
        f.gateway.send(user()).await?;
        wait_for(|| event_count(&f, "run.completed") == index + 1).await?;
        let changes: Vec<Value> = events(&f)?
            .into_iter()
            .filter(|frame| frame["event"]["type"] == "environment.changed")
            .collect();
        assert_eq!(
            changes.len(),
            before + usize::from(announced.is_some()),
            "case {index}"
        );
        if let Some(version) = announced {
            let event = &changes[changes.len() - 1]["event"];
            assert_eq!(
                event.get("version").and_then(Value::as_u64),
                version,
                "case {index}: {event}"
            );
            assert!(event.get("digest").is_none(), "never a digest: {event}");
        }
        stop(shutdown, task).await?;
    }
    f.gateway.shutdown().await?;
    Ok(())
}
