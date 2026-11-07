//! TK-U-042: a tool argument `{"seq_of": {"type": T, "from": F}}` is filled per request with the latest seq the
//! request's history names as `seq="N" type="T" from="F"` (a team read's block, ADR-0034), so a scripted lead can take
//! the files of a report whose seq depends on which helper answered first, with no timing. A history that names no such
//! message is refused loudly, naming the placeholder, never answered with a guessed seq; a malformed placeholder stops
//! the scenario from loading.
use super::support::*;
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use tokio::process::Command;

fn scenario(arguments: &Value) -> String {
    json!({"model": "fixture", "select": "prompt", "responses": [{
        "when": "Lead the team",
        "events": [{"type": "tool_call", "name": "take_files", "arguments": arguments, "split_into": 3}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    }]})
    .to_string()
}

/// A Chat request for the lead's conversation whose history holds `read`, a team read's result.
fn lead(read: &str) -> Value {
    json!({"model": "fixture", "stream": true,
        "messages": [{"role": "user", "content": "Lead the team, please."}, {"role": "tool", "content": read}],
        "tools": [{"type": "function", "function": {"name": "take_files", "parameters": {"type": "object"}}}]})
}

async fn post(server: &Binary, body: &Value) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let response = server
        .client
        .post(format!("{}/v1/chat/completions", server.base))
        .bearer_auth(TOKEN)
        .json(body)
        .send()
        .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

/// The tool call's arguments, joined from its fragments.
fn arguments(wire: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let joined: String = chunks(wire)?
        .iter()
        .flat_map(|chunk| {
            chunk["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|delta| delta["function"]["arguments"].as_str().map(str::to_owned))
        .collect();
    Ok(serde_json::from_str(&joined)?)
}

#[tokio::test]
async fn tk_u_042_seq_of_takes_the_latest_seq_the_history_names_for_that_type_and_member()
-> TestResult {
    let placeholder =
        json!({"report_seq": {"seq_of": {"type": "report", "from": "analyst"}}, "files": null});
    let server = Binary::start(&scenario(&placeholder)).await?;
    let read = "<untrusted-content source=\"team\" seq=\"4\" type=\"report\" from=\"analyst\" id=\"a\">x</untrusted-content>\n\
                <untrusted-content source=\"team\" seq=\"7\" type=\"report\" from=\"fixer\" id=\"b\">y</untrusted-content>\n\
                <untrusted-content source=\"team\" seq=\"9\" type=\"report\" from=\"analyst\" id=\"c\">z</untrusted-content>";
    let (status, wire) = post(&server, &lead(read)).await?;
    assert_eq!(status, 200, "{wire}");
    assert_eq!(
        arguments(&wire)?,
        json!({"report_seq": 9, "files": null}),
        "the analyst's latest report"
    );
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_042_a_history_without_that_message_is_refused_loudly_never_guessed() -> TestResult {
    let placeholder = json!({"report_seq": {"seq_of": {"type": "report", "from": "analyst"}}});
    let server = Binary::start(&scenario(&placeholder)).await?;
    // A report from another member, and an answer from the analyst: neither is a report from the analyst.
    let read =
        "seq=\"3\" type=\"report\" from=\"fixer\" seq=\"5\" type=\"answer\" from=\"analyst\"";
    let (status, body) = post(&server, &lead(read)).await?;
    assert_eq!(status, 422, "{body}");
    let error: Value = serde_json::from_str(&body)?;
    assert_eq!(
        error["error"]["message"],
        "seq_of: the conversation's history names no report from analyst"
    );
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_042_a_malformed_placeholder_stops_the_scenario_from_loading() -> TestResult {
    for malformed in [
        json!({"report_seq": {"seq_of": {"type": "report"}}}),
        json!({"report_seq": {"seq_of": {"type": "", "from": "analyst"}}}),
        json!({"report_seq": {"seq_of": {"type": "report", "from": "analyst", "extra": 1}}}),
        json!({"report_seq": {"seq_of": "report"}}),
    ] {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(scenario(&malformed).as_bytes())?;
        let result = tokio::time::timeout(
            tqa_testkit::deadline::scaled(Duration::from_secs(5)),
            Command::new(env!("CARGO_BIN_EXE_tqa-fake-model"))
                .arg("--scenario")
                .arg(file.path())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        assert!(!result.status.success(), "accepted: {malformed}");
    }
    Ok(())
}
