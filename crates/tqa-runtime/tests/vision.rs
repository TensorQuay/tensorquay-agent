//! RT-I-017: an attached image reaches the model through the runtime only on a vision route.
#[path = "support/fixture.rs"]
mod support;
use serde_json::Value;
use support::*;
use tqa_types::ClientFrame;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nruntime-image";

fn vision_session(vision: bool) -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["route"]["vision"] = Value::Bool(vision);
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

/// Runs one turn with `uploads/a.png` attached and returns the model request bodies.
async fn turn_bodies(vision: bool) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let f = Fixture::new(vec![answer()]).await?;
    f.gateway.set_session(vision_session(vision)?)?;
    f.gateway.set_bound(true);
    let uploads = f.directory.path().join("workspace/uploads");
    std::fs::create_dir(&uploads)?;
    std::fs::write(uploads.join("a.png"), PNG)?;
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway
        .send(ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            text: "what is in this picture?".into(),
            attachments: vec!["uploads/a.png".into()],
            host_keys: Vec::new(),
        })
        .await?;
    f.wait_event("run.completed").await?;
    stop(shutdown, task).await?;
    Ok(f.gateway
        .model_requests()
        .into_iter()
        .map(|request| request.body)
        .collect())
}

fn user_content(body: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    let messages = body["messages"].as_array().ok_or("messages")?;
    let user = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .ok_or("no user message")?;
    Ok(user["content"].clone())
}

#[tokio::test]
async fn rt_i_017_vision_route_receives_the_image_as_a_data_url() -> TestResult {
    let bodies = turn_bodies(true).await?;
    assert_eq!(bodies.len(), 1);
    let content = user_content(&bodies[0])?;
    let parts = content.as_array().ok_or("content is not an array")?;
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["type"], "image_url");
    let url = parts[1]["image_url"]["url"].as_str().ok_or("url")?;
    assert!(url.starts_with("data:image/png;base64,"));
    assert_eq!(parts.len(), 2);
    Ok(())
}

#[tokio::test]
async fn rt_i_017_non_vision_route_receives_no_image() -> TestResult {
    let bodies = turn_bodies(false).await?;
    assert_eq!(bodies.len(), 1);
    assert!(user_content(&bodies[0])?.is_string());
    assert!(!serde_json::to_string(&bodies[0])?.contains("image_url"));
    Ok(())
}
