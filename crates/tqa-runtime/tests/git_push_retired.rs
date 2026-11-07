//! RT-I-040 (ADR-0037 §7, the first of two releases): `git_push` is retired. A run is offered no `git_push` even with
//! everything that once brought it (a workspace with a clone origin, the egress proxy and a shell); a task pushes with
//! plain git and the person's key instead. The relay gets nothing. RT-I-016's push cases went with the tool.
#[path = "../../tqa-tools/tests/support/git_relay.rs"]
#[allow(dead_code)]
mod relay;
#[path = "support/fixture.rs"]
mod support;
use relay::RelayServer;
use serde_json::json;
use std::error::Error;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_runtime::{Runtime, RuntimeError};
use tqa_types::ClientFrame;

type Running = (
    CancellationToken,
    tokio::task::JoinHandle<Result<(), RuntimeError>>,
);
const REMOTE: &str = "https://git.example/repo.git";

/// The session of a workspace cloned from `REMOTE`, in an environment with a shell.
fn configured() -> Result<ClientFrame, Box<dyn Error>> {
    let mut frame = serde_json::to_value(session()?)?;
    frame["workspace"]["clone"] = json!({"url":REMOTE});
    frame["environment"] = json!({"template":"custom", "digest":format!("sha256:{}","a".repeat(64)),
        "capabilities":["shell"]});
    Ok(ClientFrame::parse(&serde_json::to_vec(&frame)?)?)
}

fn launch(f: &Fixture, proxy: String) -> Result<Running, Box<dyn Error>> {
    let runtime = Runtime::new(f.deps_for_egress(Some(proxy))?)?;
    let token = CancellationToken::new();
    let cancel = token.clone();
    f.gateway.set_bound(true);
    Ok((
        token,
        tokio::spawn(async move { runtime.run(cancel).await }),
    ))
}

// TEST: RT-I-040
#[tokio::test]
async fn rt_i_040_a_run_is_offered_no_git_push_even_with_a_clone_origin_the_proxy_and_a_shell()
-> TestResult {
    let server = RelayServer::start(vec![]).await?;
    let f = Fixture::new(vec![answer()]).await?;
    // A workspace already there is never cloned again: the run starts at once.
    std::fs::write(f.directory.path().join("workspace/existing"), "keep")?;
    f.gateway.set_session(configured()?)?;
    let (token, task) = launch(&f, server.endpoint.as_str().into())?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    let completed = f.wait_event("run.completed").await;
    stop(token, task).await?;
    completed?;
    let requests = f.gateway.model_requests();
    let tools = requests[0].body["tools"].as_array().ok_or("tools")?;
    assert!(!tools.is_empty(), "the run has its other tools");
    assert!(
        !tools
            .iter()
            .any(|tool| tool["function"]["name"] == "git_push"),
        "no git_push in the run's catalogue"
    );
    assert!(server.requests().is_empty(), "the relay got nothing");
    f.gateway.shutdown().await?;
    server.finish().await?;
    Ok(())
}
