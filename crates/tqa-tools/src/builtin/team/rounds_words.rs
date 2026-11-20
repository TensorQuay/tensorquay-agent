//! `start_member`'s words for bringing a finished helper back (ADR-0034's rounds amendment, A3).
use serde_json::{Value, json};

/// The `files` parameter: what a helper brought back works on.
pub(super) fn files_parameter() -> Value {
    json!({"type": ["string", "null"], "enum": ["keep", "fresh_copy", null],
        "description": "Only when you bring a finished helper back by its name: keep (the default) leaves its copy as it is; fresh_copy starts from the lead's files now and discards this helper's unreported changes."})
}

/// A bring-back's answer, said first as one ("Brought back researcher for round 2 (its copy kept)"); `None` for a new
/// helper.
pub(super) fn brought_back(answer: &Value) -> Option<String> {
    if answer["brought_back"] != true {
        return None;
    }
    let member = &answer["member"];
    let name = member["name"].as_str().unwrap_or("the helper");
    let files = match (member["workspace"].as_str(), answer["files"].as_str()) {
        (Some("my_files"), _) => "on your files",
        (_, Some("fresh_copy")) => {
            "a fresh copy of your files; its unreported changes were discarded"
        }
        _ => "its copy kept",
    };
    Some(format!(
        "Brought back {name} for round {} ({files}); its brief is message {}. It reports to you.",
        answer["round"], answer["seq"]
    ))
}
