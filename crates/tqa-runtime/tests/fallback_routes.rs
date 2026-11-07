//! RT-I-018: the runtime reaches admin-declared fallback routes only through their relay paths.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, Event, ServerFrame};

fn session_with_backup() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    let mut backup = value["route"].clone();
    backup["name"] = json!("backup");
    backup["model"] = json!("backup-model");
    backup["relay_path"] = json!("/relay/r/backup");
    value["fallback_routes"] = Value::Array(vec![backup]);
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

#[tokio::test]
async fn rt_i_018_a_long_retry_after_moves_the_run_to_the_fallback_relay_path() -> TestResult {
    let limited = Script::new(vec![Scripted::Status {
        code: 429,
        body: "busy".into(),
        retry_after: Some("90".into()),
    }]);
    let f = Fixture::new(vec![limited, answer()]).await?;
    f.gateway.set_session(session_with_backup()?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    stop(shutdown, task).await?;
    let paths: Vec<String> = f
        .gateway
        .requests()
        .into_iter()
        .filter(|request| request.path.starts_with("/relay/"))
        .map(|request| request.path)
        .collect();
    assert_eq!(
        paths,
        vec![
            "/relay/r/local/chat/completions".to_owned(),
            "/relay/r/backup/chat/completions".to_owned()
        ]
    );
    let models: Vec<Value> = f
        .gateway
        .model_requests()
        .into_iter()
        .map(|request| request.body["model"].clone())
        .collect();
    assert_eq!(models, vec![json!("fake-model"), json!("backup-model")]);
    let switched = f.gateway.stored().into_iter().any(|frame| {
        matches!(frame, ServerFrame::Event { event: Event::RouteFallback { ref from, ref to, ref reason }, .. }
            if from == "local" && to == "backup" && reason == "rate_limited")
    });
    assert!(switched, "the switch is journaled and stored");
    Ok(())
}
