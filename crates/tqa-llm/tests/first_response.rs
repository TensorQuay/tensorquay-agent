//! LLM-U-048: a route's first-response bound: how long the relay (and so the runtime) waits for the model's first byte.
//! A CPU model server reads a long prompt for a minute or more before it answers (a local Ollama on CPU with default settings), so the
//! bound is a route setting, `first_response_secs` (5 to 900 s). Unset, it is 300 s for a local route and 60 s
//! otherwise; the gateway decides which routes are local, so the route only takes that answer.
use std::{error::Error, time::Duration};
use tqa_llm::{FIRST_RESPONSE_LOCAL, FIRST_RESPONSE_REMOTE, Registry, Route};

type TestResult = Result<(), Box<dyn Error>>;

fn route(setting: Option<&str>) -> Result<Route, Box<dyn Error>> {
    let setting = setting.map_or_else(String::new, |value| {
        format!("first_response_secs={value}\n")
    });
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='r'\nprotocol='openai_chat'\nbase_url='http://127.0.0.1:11434/v1'\nmodel='m'\n{setting}"
    ))?;
    Ok(registry.get("r").ok_or("missing route")?.clone())
}

// TEST: LLM-U-048
#[test]
fn llm_u_048_unset_the_bound_suits_where_the_model_runs() -> TestResult {
    assert_eq!(FIRST_RESPONSE_LOCAL, Duration::from_mins(5));
    assert_eq!(FIRST_RESPONSE_REMOTE, Duration::from_mins(1));
    let route = route(None)?;
    assert_eq!(route.first_response_secs, None);
    assert_eq!(route.first_response(true), FIRST_RESPONSE_LOCAL);
    assert_eq!(route.first_response(false), FIRST_RESPONSE_REMOTE);
    Ok(())
}

// TEST: LLM-U-048
#[test]
fn llm_u_048_a_set_bound_wins_wherever_the_model_runs() -> TestResult {
    for value in [5, 120, 900] {
        let route = route(Some(&value.to_string()))?;
        assert_eq!(route.first_response_secs, Some(value));
        for local in [true, false] {
            assert_eq!(
                route.first_response(local),
                Duration::from_secs(u64::from(value))
            );
        }
    }
    Ok(())
}

// TEST: LLM-U-048
#[test]
fn llm_u_048_a_bound_out_of_range_is_refused_by_name() {
    for value in ["0", "4", "901", "86400"] {
        let error = route(Some(value)).err().map(|error| error.to_string());
        assert!(
            error
                .as_deref()
                .is_some_and(|text| text.contains("first_response_secs")),
            "{value}: {error:?}"
        );
    }
}
