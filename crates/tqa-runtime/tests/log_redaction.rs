//! SEC-007 (runtime half): key-shaped strings in tool output stay out of the runtime's logs at its default verbosity
//! (INFO) and out of its error texts. A run reads a workspace file holding them; the provider then refuses the next
//! request with an error that echoes them back, as providers do with the input they reject. Tool output itself is not
//! redacted: it reaches the model and the record through events by design.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use std::sync::Mutex;
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{Event, ServerFrame};

/// Key shapes a tool might print: a provider key, a Git host token, a cloud secret and a bearer token. Synthetic, and
/// assembled here so the source holds no token-format literal (the secret scan refuses those in any file).
fn keys() -> [String; 4] {
    let body = "Zq9vXk2LmPq7RtYw3NcBd8FhJs6GaUe4TiOy1WxVb5Mn0KlPo";
    [
        ["sk", "-ant-api03-", body].concat(),
        ["gh", "p_", &body[..36]].concat(),
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzq9XkQ2vLm".to_owned(),
        [
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiJ0ZXN0LXVzZXIifQ",
            "c2lnbmF0dXJlLWZvci10ZXN0",
        ]
        .join("."),
    ]
}

#[tokio::test]
async fn sec_007_key_shaped_tool_output_stays_out_of_logs_and_error_texts() -> TestResult {
    let keys = keys();
    let read = Script::new(vec![Scripted::ToolCall {
        name: "read_file".into(),
        arguments_json: json!({"path":"notes.txt","start_line":null,"end_line":null,"cursor":null})
            .to_string(),
        split_into: 1,
    }]);
    let echoed = format!(
        "{{\"error\":{{\"message\":\"Invalid request near '{}' and Authorization: Bearer {}\"}}}}",
        keys[0], keys[3]
    );
    let refused = Script::new(vec![Scripted::Status {
        code: 400,
        body: echoed,
        retry_after: None,
    }]);
    let f = Fixture::new(vec![read, refused]).await?;
    std::fs::write(
        f.directory.path().join("workspace/notes.txt"),
        format!(
            "ANTHROPIC_API_KEY={}\nGITHUB_TOKEN={}\naws_secret_access_key = {}\nAuthorization: Bearer {}\n",
            keys[0], keys[1], keys[2], keys[3]
        ),
    )?;
    let log_path = f.directory.path().join("runtime.log");
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(std::fs::File::create(&log_path)?))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.failed").await?;
    stop(shutdown, task).await?;

    let stored = f.gateway.stored();
    // By design the output reaches the record, and so the next model request.
    let output = stored
        .iter()
        .find_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::ToolResult { content, .. },
                ..
            } => Some(content.clone()),
            _ => None,
        })
        .ok_or("no tool.result")?;
    assert!(
        keys.iter().all(|key| output.contains(key)),
        "the tool read the file: {output}"
    );
    // Error texts: the run's failure, and any error frame, name none of them.
    let failures: Vec<String> = stored
        .iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event {
                event: event @ Event::RunFailed { .. },
                ..
            } => Some(serde_json::to_string(event).unwrap_or_default()),
            ServerFrame::Error { .. } => Some(serde_json::to_string(frame).unwrap_or_default()),
            _ => None,
        })
        .collect();
    assert!(!failures.is_empty(), "the refused request fails the run");
    let logs = std::fs::read_to_string(&log_path)?;
    for key in &keys {
        for failure in &failures {
            assert!(
                !failure.contains(key),
                "an error text holds a key: {failure}"
            );
        }
        assert!(!logs.contains(key), "the runtime log holds a key:\n{logs}");
    }
    Ok(())
}
