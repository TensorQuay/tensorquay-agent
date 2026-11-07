//! GW-U-067: a failed probe step says why the model's answer stopped, so a cut at the probe's own output cap is read,
//! not inferred: `finish: <reason>` in the failure's detail after an answer that isn't the step's, and how the strict
//! answer lost the line break when the strict-string check fails. GW-U-068: a call whose arguments the output limit
//! cut is named as cut, with the tokens the server reported, instead of as invalid arguments.

use super::probe_tests::{
    NONCE, TestResult, call, limiter, lines, probe, route, start_model, usage,
};
use tqa_llm::FinishReason;
use tqa_testkit::{Script, Scripted};

// TEST: GW-U-067
#[tokio::test]
async fn gw_u_067_a_step_that_fails_after_an_answer_names_its_finish() -> TestResult {
    // No call: the answer ran out at the cap.
    let cut = vec![Script::new(vec![
        Scripted::Text("Let me think about which tool".into()),
        Scripted::Finish(FinishReason::Length),
        usage(),
    ])];
    // The round trip answered with another call instead of text.
    let again = vec![
        Script::new(vec![call(NONCE), usage()]),
        Script::new(vec![call(NONCE), usage()]),
    ];
    for (scripts, code, detail) in [
        (cut, "no_tool_call", "finish: length"),
        (again, "round_trip_failed", "finish: tool_calls"),
    ] {
        let server = start_model(scripts).await?;
        let route = route("openai_chat", server.base_url(), "")?;
        let observed = probe(&route, limiter(&route)).await?;
        let failure = observed.failure.as_ref().ok_or("no failure")?;
        assert_eq!(
            (failure.code, failure.detail.as_deref()),
            (code, Some(detail)),
            "{observed:?}"
        );
        server.shutdown().await?;
    }
    Ok(())
}

// TEST: GW-U-067
#[tokio::test]
async fn gw_u_067_a_lost_line_break_says_how_the_strict_answer_lost_it() -> TestResult {
    let value = format!("{NONCE}\n{NONCE}");
    let scripts = vec![
        Script::new(vec![call(NONCE), usage()]),
        Script::new(vec![Scripted::Text("ok".into()), usage()]),
        // Strict: the answer is cut at the cap; without strict it keeps the line break.
        Script::new(vec![
            lines(&value),
            Scripted::Finish(FinishReason::Length),
            usage(),
        ]),
        Script::new(vec![lines(&value), usage()]),
    ];
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!(failure.code, "strict_line_breaks_lost");
    assert_eq!(
        failure.detail.as_deref(),
        Some("strict answer: finish: length")
    );
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-068
#[tokio::test]
async fn gw_u_068_a_call_cut_at_the_output_limit_is_named_as_cut() -> TestResult {
    // The call's arguments stop mid-JSON because the answer ran out at the cap (seen live at 64 tokens).
    let scripts = vec![Script::new(vec![
        Scripted::ToolCall {
            name: "probe_echo".into(),
            arguments_json: format!("{{\"value\":\"{}", &NONCE[..4]),
            split_into: 2,
        },
        Scripted::Finish(FinishReason::Length),
        usage(),
    ])];
    let server = start_model(scripts).await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!(
        failure.detail.as_deref(),
        Some("tool 0 arguments were cut at the output limit (5 tokens)"),
        "{observed:?}"
    );
    server.shutdown().await?;
    Ok(())
}
