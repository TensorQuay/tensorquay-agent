use super::support::*;
use serde_json::Value;
use std::collections::BTreeSet;

#[tokio::test]
async fn web_e2e_018_long_demo_streams_a_multistep_markdown_answer() -> TestResult {
    let source = long_demo()?;
    let scenario: Value = serde_json::from_str(&source)?;
    let replies = scenario["responses"].as_array().ok_or("responses")?;
    let mut fast = scenario.clone();
    for reply in fast["responses"].as_array_mut().ok_or("responses")? {
        reply["delay_ms"] = 0.into();
    }
    let server = Binary::start(&fast.to_string()).await?;
    let mut scheduled_ms = 0;
    let mut kinds = BTreeSet::new();
    let mut calls = 0;
    let mut final_text = String::new();
    for (turn, reply) in replies.iter().enumerate() {
        let mut body = request(turn);
        body["stream_options"] = serde_json::json!({"include_usage":true});
        let records = chunks(&server.complete(body).await?)?;
        scheduled_ms += records.len() as u64 * reply["delay_ms"].as_u64().ok_or("delay")?;
        for event in reply["events"]
            .as_array()
            .ok_or("events")?
            .iter()
            .filter(|event| event["type"] == "tool_call")
        {
            calls += 1;
            kinds.insert(event["name"].as_str().ok_or("name")?);
        }
        assert!(
            records
                .iter()
                .any(|r| r["choices"][0]["delta"].get("content").is_some())
        );
        if turn + 1 == replies.len() {
            final_text = records
                .iter()
                .filter_map(|r| r["choices"][0]["delta"]["content"].as_str())
                .collect();
        }
    }
    assert!(calls >= 10);
    assert_eq!(
        kinds,
        BTreeSet::from([
            "write_file",
            "edit_file",
            "read_file",
            "grep",
            "glob",
            "shell"
        ])
    );
    assert!(
        (7500..=8500).contains(&scheduled_ms),
        "scheduled {scheduled_ms} ms"
    );
    assert_markdown(&final_text);
    assert!(source.contains("reasoning"));
    server.stop().await?;
    Ok(())
}

fn assert_markdown(final_text: &str) {
    assert!(
        (2800..=3400).contains(&final_text.len()),
        "{} bytes",
        final_text.len()
    );
    assert_eq!(
        final_text
            .lines()
            .filter(|line| line.starts_with("## "))
            .count(),
        2
    );
    assert_eq!(
        final_text
            .lines()
            .filter(|line| line.starts_with("```"))
            .count(),
        4
    );
    assert!(final_text.contains("\n- "));
    assert!(final_text.contains("| --- |"));
    assert!(final_text.contains("`demo/"));
    assert!(
        final_text
            .split("```")
            .skip(1)
            .step_by(2)
            .any(|fence| fence.lines().any(|line| line.len() > 400))
    );
}
