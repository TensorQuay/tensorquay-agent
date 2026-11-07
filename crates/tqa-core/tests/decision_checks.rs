//! CORE-U-063: the loop's checks of an engine's answer, public so that `ext-check decision`, the conformance suite an
//! engine's author runs (ADR-0013 point 6), applies the very same rules and can't drift from the loop. A `call_model`
//! keeps to the step's routes and tools, without repeats; an `act` names a step tool with an object its schema accepts;
//! a selection names only the response's calls, each once; a stop reason is 1 to 200 characters without control
//! characters.
use serde_json::json;
use std::collections::BTreeSet;
use tqa_core::{
    BudgetUse, Decision, PlannedAction, RunState, check_act, check_call_model, check_selection,
    valid_stop_reason,
};
use tqa_llm::ToolCall;
use tqa_types::DecisionFallbackReason;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn state() -> Result<RunState, Box<dyn std::error::Error>> {
    Ok(RunState {
        run_id: "r-1".parse()?,
        step: 1,
        attempt: 1,
        route: "local".into(),
        routes: vec!["local".into(), "cheap".into()],
        tools: vec!["list_dir".into(), "read_file".into()],
        budget: BudgetUse {
            steps_used: 0,
            max_steps: 30,
            output_tokens_used: 0,
            max_output_tokens: 1000,
        },
        previous_calls: vec![],
        transcript: None,
    })
}
fn call_model(route: &str, tools: &[&str]) -> Decision {
    Decision::CallModel {
        route: route.into(),
        tool_subset: tools.iter().map(|&tool| tool.to_owned()).collect(),
    }
}
fn act(tool: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: "de-1-1".into(),
        name: tool.into(),
        arguments: arguments.into(),
    }
}
fn planned(ids: &[&str]) -> Vec<PlannedAction> {
    ids.iter()
        .map(|&id| PlannedAction {
            call_id: id.to_owned(),
        })
        .collect()
}

// TEST: CORE-U-063
#[test]
fn core_u_063_a_call_model_keeps_to_the_steps_routes_and_tools() -> TestResult {
    let state = state()?;
    assert_eq!(
        check_call_model(&state, call_model("cheap", &["read_file"])),
        Ok((BTreeSet::from(["read_file".to_owned()]), "cheap".to_owned()))
    );
    assert_eq!(
        check_call_model(&state, call_model("elsewhere", &[])),
        Err(DecisionFallbackReason::RouteNotAllowed)
    );
    for tools in [&["write_file"][..], &["read_file", "read_file"][..]] {
        assert_eq!(
            check_call_model(&state, call_model("local", tools)),
            Err(DecisionFallbackReason::ToolNotAllowed),
            "{tools:?}"
        );
    }
    assert_eq!(
        check_call_model(&state, Decision::Compact),
        Err(DecisionFallbackReason::Invalid),
        "only a call_model is checked here"
    );
    Ok(())
}

// TEST: CORE-U-063
#[test]
fn core_u_063_an_act_names_a_step_tool_with_arguments_its_schema_accepts() -> TestResult {
    let state = state()?;
    let schema = |tool: &str, arguments: &serde_json::Value| {
        tool == "read_file"
            && arguments
                .get("path")
                .is_some_and(serde_json::Value::is_string)
    };
    assert_eq!(
        check_act(&state, &act("read_file", r#"{"path":"a.txt"}"#), schema),
        Ok(json!({"path": "a.txt"}))
    );
    assert_eq!(
        check_act(&state, &act("write_file", r#"{"path":"a.txt"}"#), schema),
        Err(DecisionFallbackReason::ToolNotAllowed)
    );
    for arguments in [r#"{"path":1}"#, "[]", "not json"] {
        assert_eq!(
            check_act(&state, &act("read_file", arguments), schema),
            Err(DecisionFallbackReason::Schema),
            "{arguments}"
        );
    }
    Ok(())
}

// TEST: CORE-U-063
#[test]
fn core_u_063_a_selection_names_only_the_responses_calls_each_once() {
    let original = BTreeSet::from(["c1".to_owned(), "c2".to_owned()]);
    assert_eq!(
        check_selection(&original, &planned(&["c2"])),
        Ok(BTreeSet::from(["c2".to_owned()]))
    );
    assert_eq!(
        check_selection(&original, &planned(&[])),
        Ok(BTreeSet::new())
    );
    for ids in [&["c3"][..], &["c1", "c1"][..]] {
        assert_eq!(
            check_selection(&original, &planned(ids)),
            Err(DecisionFallbackReason::Invalid),
            "{ids:?}"
        );
    }
}

// TEST: CORE-U-063
#[test]
fn core_u_063_a_stop_reason_is_one_to_two_hundred_plain_characters() {
    assert!(valid_stop_reason("the task is out of scope"));
    assert!(valid_stop_reason(&"é".repeat(200)));
    assert!(!valid_stop_reason(""));
    assert!(!valid_stop_reason(&"x".repeat(201)));
    assert!(!valid_stop_reason("line one\nline two"));
}
