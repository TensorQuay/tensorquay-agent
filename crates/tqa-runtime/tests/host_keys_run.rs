//! RT-I-037, through the runtime (ADR-0037 §3): the keys on the delivered `user.message` reach that run's commands as
//! variables and its prompt as names and addresses, never the prompt as placeholders; a run without keys has neither.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_runtime::Runtime;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, Event, HostKeyGrant, ServerFrame};

const PLACEHOLDER: &str = "tqa_ph_run0000000000000000000000000000000000000000";

fn keyed() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    Ok(ClientFrame::UserMessage {
        lead_brief: None,
        team_wake: None,
        text: "question".into(),
        attachments: vec![],
        host_keys: vec![HostKeyGrant::new(
            "GITHUB_WORK".into(),
            ("github.com".into(), 443, Some("/acme".into())),
            ("basic".into(), Some("x-access-token".into())),
            PLACEHOLDER.into(),
        )?],
    })
}

const CA: &str =
    "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUQ0E=\n-----END CERTIFICATE-----\n";

/// The fixture's session, with the install CA when given.
fn session_with(host_ca: Option<&str>) -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut frame = serde_json::to_value(session()?)?;
    if let Some(host_ca) = host_ca {
        frame["host_ca"] = json!(host_ca);
    }
    Ok(ClientFrame::parse(frame.to_string().as_bytes())?)
}

/// The shell's `env` output of one run started by `message`, and the system prompt of its first model request.
async fn run(message: ClientFrame) -> Result<(String, String), Box<dyn std::error::Error>> {
    run_in(message, None).await
}

async fn run_in(
    message: ClientFrame,
    host_ca: Option<&str>,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let f = Fixture::new(vec![
        Script::new(vec![Scripted::ToolCall {
            name: "shell".into(),
            arguments_json: json!({"command": "env; tail -n 3 \"${SSL_CERT_FILE:-/dev/null}\"", "cwd": null, "timeout_ms": null}).to_string(),
            split_into: 1,
        }]),
        answer(),
    ])
    .await?;
    f.gateway.set_session(session_with(host_ca)?)?;
    let runtime = Runtime::new(f.deps()?)?;
    let shutdown = CancellationToken::new();
    let cancel = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(cancel).await });
    f.gateway.set_bound(true);
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(message).await?;
    let result = f.wait_event("run.completed").await;
    stop(shutdown, task).await?;
    result?;
    let output = f
        .gateway
        .stored()
        .iter()
        .find_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::ToolResult { content, .. },
                ..
            } => Some(content.clone()),
            _ => None,
        })
        .ok_or("no tool result")?;
    let requests = f.gateway.model_requests();
    let prompt = requests[0].body.to_string();
    f.gateway.shutdown().await?;
    Ok((output, prompt))
}

// TEST: RT-I-037
#[tokio::test]
async fn rt_i_037_a_runs_keys_reach_its_commands_and_its_prompt_as_names() -> TestResult {
    let (output, prompt) = run(keyed()?).await?;
    assert!(
        output.contains(&format!("GITHUB_WORK={PLACEHOLDER}")),
        "{output}"
    );
    assert!(
        output.contains("credential.https://github.com/acme.helper"),
        "{output}"
    );
    assert!(
        prompt.contains("- GITHUB_WORK: https://github.com/acme (Basic, user x-access-token)"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("tqa_ph_"),
        "a placeholder never reaches the model's prompt"
    );

    let (output, prompt) = run(user()).await?;
    assert!(
        !output.contains("GITHUB_WORK") && !prompt.contains("Keys:"),
        "{output}"
    );
    Ok(())
}

// TEST: RT-I-037
#[tokio::test]
async fn rt_i_037_every_command_trusts_the_install_ca_through_one_bundle() -> TestResult {
    let (output, _) = run_in(user(), Some(CA)).await?;
    let variables: std::collections::BTreeMap<&str, &str> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let bundle = variables.get("SSL_CERT_FILE").ok_or("no SSL_CERT_FILE")?;
    assert!(bundle.ends_with("/state/host-ca-bundle.pem"), "{bundle}");
    for name in tqa_tools::builtin::CA_BUNDLE_VARIABLES {
        assert_eq!(variables.get(name), Some(bundle), "{name}");
    }
    // The bundle ends with the install CA, after the image's own roots.
    assert!(output.trim_end().ends_with(CA.trim_end()), "{output}");

    let (output, _) = run_in(user(), None).await?;
    assert!(!output.contains("SSL_CERT_FILE"), "{output}");
    Ok(())
}

/// The stored frames of a run that started on `message` and went no further: what a runtime finds after a restart.
async fn interrupted(message: ClientFrame) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let f = Fixture::new(vec![answer()]).await?;
    let runtime = Runtime::new(f.deps()?)?;
    let shutdown = CancellationToken::new();
    let cancel = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(cancel).await });
    f.gateway.set_bound(true);
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(message).await?;
    let result = f.wait_event("run.completed").await;
    stop(shutdown, task).await?;
    result?;
    let stored = f.gateway.stored();
    f.gateway.shutdown().await?;
    let (mut user, mut started) = (false, false);
    Ok(stored
        .into_iter()
        .take_while(|frame| {
            let before = user && started;
            if let ServerFrame::Event { event, .. } = frame {
                user |= matches!(event, Event::MessageUser { .. });
                started |= matches!(event, Event::RunStarted { .. });
            }
            !before
        })
        .collect())
}

// TEST: RT-I-037
#[tokio::test]
async fn rt_i_037_a_resumed_run_gets_the_keys_its_registration_brings() -> TestResult {
    let history = interrupted(keyed()?).await?;
    let f = Fixture::with_history(
        vec![
            Script::new(vec![Scripted::ToolCall {
                name: "shell".into(),
                arguments_json: json!({"command": "env", "cwd": null, "timeout_ms": null})
                    .to_string(),
                split_into: 1,
            }]),
            answer(),
        ],
        history,
    )
    .await?;
    // The gateway minted new placeholders for this sandbox and sends them with the configuration.
    let resumed = PLACEHOLDER.replace("run", "rsm");
    let mut session = serde_json::to_value(session()?)?;
    let ClientFrame::UserMessage { host_keys, .. } = keyed()? else {
        return Err("keyed is a user.message".into());
    };
    let mut grant = serde_json::to_value(&host_keys[0])?;
    grant["placeholder"] = json!(resumed);
    session["run_host_keys"] = json!([grant]);
    f.gateway
        .set_session(ClientFrame::parse(session.to_string().as_bytes())?)?;
    let runtime = Runtime::new(f.deps()?)?;
    let shutdown = CancellationToken::new();
    let cancel = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(cancel).await });
    f.gateway.set_bound(true);
    let result = f.wait_event("run.completed").await;
    stop(shutdown, task).await?;
    result?;
    let stored = f.gateway.stored();
    assert!(stored.iter().any(|frame| matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunResumed { .. },
            ..
        }
    )));
    let output = stored
        .iter()
        .find_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::ToolResult { content, .. },
                ..
            } => Some(content.clone()),
            _ => None,
        })
        .ok_or("no tool result")?;
    assert!(
        output.contains(&format!("GITHUB_WORK={resumed}")),
        "{output}"
    );
    f.gateway.shutdown().await?;
    Ok(())
}
