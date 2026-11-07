//! RT-I-030: the channel tools are offered to a run only when its conversation belongs to a channel at run start
//! (ADR-0014; the runtime asks `GET /runtime/channels` at every run start), and only if the profile's allow-list lets
//! them in. An older gateway without the route, or no channel, gives none.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use support::*;
use tqa_types::ClientFrame;

const CHANNEL_TOOLS: [&str; 4] = ["channel_post", "channel_read", "channel_wait", "task_claim"];

async fn offered(f: &Fixture) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    stop(shutdown, task).await?;
    let body: Value = f
        .gateway
        .model_requests()
        .into_iter()
        .next()
        .ok_or("no model request")?
        .body;
    Ok(body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .filter(|name| CHANNEL_TOOLS.contains(&name.as_str()))
        .collect())
}

fn membership() -> Value {
    json!({"id":"ch-1","name":"Refactor","purpose":"Split the parser","role":"member","rule":"majority","state":"open"})
}

#[tokio::test]
async fn rt_i_030_a_member_conversations_run_gets_the_four_tools() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.set_channels(Some(vec![membership()]));
    let mut names = offered(&f).await?;
    names.sort();
    assert_eq!(names, CHANNEL_TOOLS);
    // Asked at every run start (the supervisor's bootstrap run included), always with the runtime token, and before
    // the model sees the tools.
    let paths: Vec<(String, bool)> = f
        .gateway
        .requests()
        .into_iter()
        .map(|request| (request.path, request.authenticated))
        .collect();
    let first_model = paths
        .iter()
        .position(|(path, _)| path.starts_with("/relay/"))
        .ok_or("no model request")?;
    let asked: Vec<&(String, bool)> = paths[..first_model]
        .iter()
        .filter(|(path, _)| path == "/runtime/channels")
        .collect();
    assert!(!asked.is_empty(), "{paths:?}");
    assert!(
        asked.iter().all(|(_, authenticated)| *authenticated),
        "{paths:?}"
    );
    Ok(())
}

#[tokio::test]
async fn rt_i_030_no_channel_or_an_older_gateway_gives_none() -> TestResult {
    for channels in [None, Some(vec![])] {
        let f = Fixture::in_memory(vec![answer()]).await?;
        f.gateway.set_channels(channels.clone());
        assert!(offered(&f).await?.is_empty(), "{channels:?}");
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_030_a_profile_without_them_leaves_them_out() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.set_channels(Some(vec![membership()]));
    let mut session = serde_json::to_value(session()?)?;
    session["profile"] = json!({"name": "reader", "tools": ["read_file"]});
    f.gateway
        .set_session(ClientFrame::parse(&serde_json::to_vec(&session)?)?)?;
    assert!(offered(&f).await?.is_empty());
    Ok(())
}
