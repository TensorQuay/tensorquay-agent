//! Turns the shared demo fixture into scripts for the real container run.
use serde_json::Value;
use std::{error::Error, path::Path};
use tqa_testkit::{Script, Scripted};

pub fn scripts() -> Result<Vec<Script>, Box<dyn Error>> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tqa-testkit/scenarios/demo-long.json");
    let document: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    document["responses"]
        .as_array()
        .ok_or("responses")?
        .iter()
        .map(|reply| {
            let events = reply["events"]
                .as_array()
                .ok_or("events")?
                .iter()
                .map(event)
                .collect::<Result<_, _>>()?;
            Ok(Script::new(events))
        })
        .collect()
}

fn event(value: &Value) -> Result<Scripted, Box<dyn Error>> {
    Ok(match value["type"].as_str() {
        Some("text") => Scripted::Text(value["text"].as_str().ok_or("text")?.into()),
        Some("reasoning") => Scripted::Reasoning(value["text"].as_str().ok_or("reasoning")?.into()),
        Some("tool_call") => Scripted::ToolCall {
            name: value["name"].as_str().ok_or("name")?.into(),
            arguments_json: value["arguments"].to_string(),
            split_into: 4,
        },
        _ => return Err("unknown demo event".into()),
    })
}
