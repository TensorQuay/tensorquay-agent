//! LLM-U-031: the registry built from route records (the database path) validates exactly like the file, and
//! every refusal carries a typed reason with a stable code.
//! Also LLM-U-015: an invalid reasoning mapping (a default outside `allowed`) is refused when the registry loads.

use serde_json::{Value, json};
use std::error::Error;
use tqa_llm::{Registry, RouteConfig};

type TestResult = Result<(), Box<dyn Error>>;

fn chat() -> Value {
    json!({"name":"local","protocol":"openai_chat","base_url":"https://models.example/v1","model":"example-model",
        "auth":{"kind":"bearer"},"key_ref":"secrets/local","context_window":32768,"max_output":4096,
        "reasoning":{"field":"reasoning_effort","allowed":["low","high"],"default":"low"},
        "extra_headers":{"x-route":"on"},"extra_body":{"service_tier":"auto"},"fallbacks":["spare"]})
}
fn spare() -> Value {
    json!({"name":"spare","protocol":"anthropic_messages","base_url":"https://other.example/v1","model":"m",
        "auth":{"kind":"none"},"max_output":4096})
}
fn configs(documents: Vec<Value>) -> Result<Vec<RouteConfig>, serde_json::Error> {
    documents.into_iter().map(serde_json::from_value).collect()
}

#[test]
fn llm_u_031_records_build_the_same_registry_as_the_file() -> TestResult {
    let records = Registry::from_configs(configs(vec![chat(), spare()])?)?;
    let file = Registry::from_toml(&records.to_toml()?)?;
    assert_eq!(records, file);
    assert_eq!(records.names().collect::<Vec<_>>(), ["local", "spare"]);
    // A record is JSON with the models.toml fields; unknown fields are refused, as in the file.
    let mut unknown = chat();
    unknown["surprise"] = json!(true);
    assert!(serde_json::from_value::<RouteConfig>(unknown).is_err());
    Ok(())
}

/// One invalid record next to a valid one, and the reason's code the API reports.
fn reason(document: Value) -> Result<(Option<String>, String), Box<dyn Error>> {
    let error = Registry::from_configs(configs(vec![document, spare()])?)
        .err()
        .ok_or("the record was accepted")?;
    let code = error.reason().ok_or("no typed reason")?.code().to_owned();
    Ok((error.route().map(str::to_owned), code))
}

#[test]
fn llm_u_031_every_refusal_has_a_typed_reason_and_names_the_route() -> TestResult {
    let set = |field: &str, value: Value| {
        let mut document = chat();
        document[field] = value;
        document
    };
    let cases = [
        (set("name", json!("")), "route_name_invalid"),
        (set("model", json!("")), "model_empty"),
        (
            set("base_url", json!("ftp://models.example/v1")),
            "base_url_invalid",
        ),
        (set("base_url", json!("not a url")), "base_url_invalid"),
        (set("key_ref", Value::Null), "key_ref_required"),
        (
            set("auth", json!({"kind":"header","name":"bad header"})),
            "auth_header_invalid",
        ),
        (
            set("auth", json!({"kind":"query","name":"a b"})),
            "auth_query_invalid",
        ),
        (
            set(
                "reasoning",
                json!({"field":"reasoning_effort","allowed":["low"],"default":"high"}),
            ),
            "reasoning_invalid",
        ),
        (
            set("extra_body", json!({"messages":[]})),
            "extra_body_reserved",
        ),
        (
            set("extra_headers", json!({"Authorization":"x"})),
            "extra_header_credential",
        ),
        (
            set("extra_headers", json!({"bad name":"x"})),
            "extra_header_invalid",
        ),
        (set("quirks", json!(["legacy"])), "quirk_unsupported"),
        (set("strict", json!("yes")), "strict_invalid"),
        (set("strict", json!("On")), "strict_invalid"),
        (
            set("schema_keywords", json!("minimal")),
            "schema_keywords_invalid",
        ),
        (set("fallbacks", json!(["local"])), "fallback_invalid"),
        (set("fallbacks", json!(["absent"])), "fallback_invalid"),
        (set("context_window", json!(0)), "limit_out_of_range"),
        (set("max_output", json!(65536)), "limit_out_of_range"),
        (set("concurrency", json!(0)), "limit_out_of_range"),
        (set("requests_per_minute", json!(0)), "limit_out_of_range"),
    ];
    for (document, expected) in cases {
        let (route, code) = reason(document.clone())?;
        assert_eq!(code, expected, "{document}");
        let name = document["name"].as_str().unwrap_or_default();
        assert_eq!(route.as_deref(), Some(name), "{expected}");
    }
    Ok(())
}

#[test]
fn llm_u_031_protocol_specific_reasons() -> TestResult {
    let mut budget = chat();
    budget["reasoning"]["budget_tokens"] = json!({"low":2048,"high":4096});
    assert_eq!(reason(budget)?.1, "budget_invalid");
    let mut messages = spare();
    messages["name"] = json!("local");
    messages["max_output"] = Value::Null;
    assert_eq!(reason(messages.clone())?.1, "max_output_required");
    messages["max_output"] = json!(4096);
    messages["extra_headers"] = json!({"anthropic-version":"2099-01-01"});
    assert_eq!(reason(messages)?.1, "extra_header_reserved");
    Ok(())
}

#[test]
fn llm_u_031_the_file_keeps_its_message_and_line_and_gains_the_reason() -> TestResult {
    let source =
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://h/v1'\nmodel=''\n";
    let error = Registry::from_toml(source).err().ok_or("accepted")?;
    assert_eq!(error.line(), Some(1));
    assert!(
        error
            .to_string()
            .contains("route local: name and model must not be empty"),
        "{error}"
    );
    assert_eq!(
        error.reason().map(tqa_llm::RouteInvalid::code),
        Some("model_empty")
    );
    assert_eq!(error.route(), Some("local"));
    // A duplicate name is a registry error, not a route reason.
    let duplicate = Registry::from_configs(configs(vec![spare(), spare()])?)
        .err()
        .ok_or("accepted")?;
    assert!(duplicate.reason().is_none() && duplicate.to_string().contains("duplicate route name"));
    Ok(())
}

// TEST: SEC-033
#[test]
fn sec_033_only_a_configured_route_must_name_a_secret_a_process_route_may_label_its_own_credential()
-> TestResult {
    // The runtime's own route: its credential is its token file, passed explicitly, and its key_ref only a label.
    let mut document = spare();
    document["name"] = json!("runtime");
    document["auth"] = json!({"kind": "bearer"});
    document["key_ref"] = json!("runtime-token-file");
    let config: RouteConfig = serde_json::from_value(document.clone())?;
    tqa_llm::provider_for(
        tqa_llm::Route::from(config),
        Some(secrecy::SecretString::from("synthetic-token")),
        tqa_llm::HttpConfig::new(tqa_llm::StreamLimits::RECOMMENDED),
        std::sync::Arc::new(tqa_llm::SystemClock),
        std::sync::Arc::new(tqa_llm::TokioSleeper),
    )?;
    // The same record configured in a registry (the file, the admin API, the stored routes) is refused.
    let (_, code) = reason(document)?;
    assert_eq!(code, "key_ref_not_reference");
    Ok(())
}

#[test]
fn gw_u_058_the_tool_schema_settings_default_and_keep_their_values() -> TestResult {
    use tqa_llm::tool_schemas::{SchemaKeywords, StrictTools};
    // Absent: auto and full, and a record written back leaves them out.
    let registry = Registry::from_configs(configs(vec![chat(), spare()])?)?;
    let route = registry.get("local").ok_or("local")?;
    assert_eq!(
        (route.strict_tools(), route.schema_keywords()),
        (StrictTools::Auto, SchemaKeywords::Full)
    );
    let written = serde_json::to_value(RouteConfig::from(route.clone()))?;
    assert!(written.get("strict").is_none() && written.get("schema_keywords").is_none());
    for (strict, keywords, expected) in [
        ("auto", "full", (StrictTools::Auto, SchemaKeywords::Full)),
        ("on", "basic", (StrictTools::On, SchemaKeywords::Basic)),
        ("off", "full", (StrictTools::Off, SchemaKeywords::Full)),
    ] {
        let mut document = chat();
        document["strict"] = json!(strict);
        document["schema_keywords"] = json!(keywords);
        let registry = Registry::from_configs(configs(vec![document, spare()])?)?;
        let route = registry.get("local").ok_or("local")?;
        assert_eq!((route.strict_tools(), route.schema_keywords()), expected);
        // The file form reads back the same.
        let file = Registry::from_toml(&registry.to_toml()?)?;
        assert_eq!(file, registry);
    }
    // A wrong type is not a record at all.
    let mut wrong = chat();
    wrong["strict"] = json!(true);
    assert!(serde_json::from_value::<RouteConfig>(wrong).is_err());
    Ok(())
}
