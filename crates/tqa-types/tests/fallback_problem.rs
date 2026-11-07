//! PROTO-U-044: `provider.retry`'s optional `fallback_problem`, what exactly made a text-protocol reply unreadable.
//! Every kind round-trips exactly and the exported schema accepts it; absent stays absent and `reason` is unchanged;
//! an unknown kind from a newer runtime reads as `other`; a name that isn't an identifier, an over-long list and an
//! over-long name are refused.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    Event, FallbackProblem, JsonType, ProblemName, ProblemPath, ServerFrame, protocol_schema,
};

type TestResult = Result<(), Box<dyn Error>>;

fn retry(problem: Option<Value>) -> Value {
    let mut event = json!({"type":"provider.retry","attempt":1,"delay_ms":0,"reason":"fallback_parse: malformed"});
    if let Some(problem) = problem {
        event["fallback_problem"] = problem;
    }
    event
}
fn parsed(event: &Value) -> Result<ServerFrame, tqa_types::FrameError> {
    let frame = json!({"type":"event","seq":1,"run_id":"r-1","step":1,"ts":"2000-01-01T00:00:00Z","event":event});
    ServerFrame::parse(&serde_json::to_vec(&frame).unwrap_or_default())
}
fn schema_accepts(event: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    Ok(jsonschema::validator_for(&schema)?.is_valid(event))
}

#[test]
fn proto_u_044_each_kind_round_trips_and_the_schema_accepts_it() -> TestResult {
    let kinds = [
        json!({"kind":"no_name"}),
        json!({"kind":"no_arguments"}),
        json!({"kind":"not_json","cut_off":true,"line":12,"column":3}),
        json!({"kind":"not_object"}),
        json!({"kind":"extra_keys","keys":["tool"]}),
        json!({"kind":"extra_keys","keys":[]}),
        json!({"kind":"name_not_string"}),
        json!({"kind":"unknown_tool","name":"search"}),
        json!({"kind":"unknown_tool"}),
        json!({"kind":"unexpected_property","tool":"glob","property":"path","allowed":["pattern","max_results","cursor"]}),
        json!({"kind":"missing_property","tool":"write_file","property":"create_parents","expected":["boolean"]}),
        json!({"kind":"wrong_type","tool":"write_file","property":"create_parents","expected":["boolean","null"]}),
        json!({"kind":"missing_property","tool":"edit_file","at":"edits[0]","property":"new_text","expected":["string"]}),
        json!({"kind":"unexpected_property","tool":"edit_file","at":"edits[2]","property":"x","allowed":["new_text","old_text","replace_all"]}),
        json!({"kind":"wrong_type","tool":"edit_file","at":"edits[0]","property":"replace_all","expected":["boolean","null"]}),
        json!({"kind":"missing_property","tool":"t","at":"a.b[12].c","property":"d","expected":[]}),
        json!({"kind":"other_schema","tool":"write_file"}),
        json!({"kind":"too_many","count":3,"limit":1}),
        json!({"kind":"other"}),
    ];
    for problem in kinds {
        let event = retry(Some(problem.clone()));
        let decoded: Event = serde_json::from_value(event.clone())?;
        assert_eq!(
            serde_json::to_value(&decoded)?,
            event,
            "exact round trip of {problem}"
        );
        parsed(&event)?;
        assert!(schema_accepts(&event)?, "the schema accepts {problem}");
    }
    let Event::ProviderRetry {
        reason,
        fallback_problem,
        ..
    } = serde_json::from_value(retry(Some(
        json!({"kind":"missing_property","tool":"write_file","property":"create_parents","expected":["boolean"]}),
    )))?
    else {
        return Err("not a provider.retry".into());
    };
    assert_eq!(reason, "fallback_parse: malformed");
    assert_eq!(
        fallback_problem,
        Some(FallbackProblem::MissingProperty {
            tool: ProblemName::new("write_file").ok_or("name")?,
            at: None,
            property: ProblemName::new("create_parents").ok_or("name")?,
            expected: vec![JsonType::Boolean],
        })
    );
    Ok(())
}

#[test]
fn proto_u_044_absent_stays_absent() -> TestResult {
    let event = retry(None);
    let decoded: Event = serde_json::from_value(event.clone())?;
    assert_eq!(serde_json::to_value(&decoded)?, event);
    let Event::ProviderRetry {
        fallback_problem, ..
    } = decoded
    else {
        return Err("not a provider.retry".into());
    };
    assert_eq!(fallback_problem, None);
    assert!(schema_accepts(&event)?);
    Ok(())
}

#[test]
fn proto_u_044_an_unknown_kind_reads_as_other() -> TestResult {
    let event = retry(Some(json!({"kind":"from_a_newer_runtime","detail":3})));
    let Event::ProviderRetry {
        fallback_problem, ..
    } = serde_json::from_value(event.clone())?
    else {
        return Err("not a provider.retry".into());
    };
    assert_eq!(fallback_problem, Some(FallbackProblem::Other));
    parsed(&event)?;
    Ok(())
}

#[test]
fn proto_u_044_names_and_lists_are_bounded() -> TestResult {
    let refused = [
        json!({"kind":"unknown_tool","name":"a b"}),
        json!({"kind":"unknown_tool","name":"sk-live\"}"}),
        json!({"kind":"unknown_tool","name":"9lives"}),
        json!({"kind":"unknown_tool","name":"x".repeat(65)}),
        json!({"kind":"extra_keys","keys":["a","b","c","d","e","f","g","h","i"]}),
        json!({"kind":"unexpected_property","tool":"glob","allowed":["a","b","c","d","e","f","g","h","i"]}),
        json!({"kind":"missing_property","tool":"write_file","property":"","expected":[]}),
        // A place is names and indices, the first a name, at most four segments.
        json!({"kind":"missing_property","tool":"t","at":"[0]","property":"p","expected":[]}),
        json!({"kind":"missing_property","tool":"t","at":"a b","property":"p","expected":[]}),
        json!({"kind":"missing_property","tool":"t","at":"a[x]","property":"p","expected":[]}),
        json!({"kind":"missing_property","tool":"t","at":"a.b.c.d.e","property":"p","expected":[]}),
        json!({"kind":"missing_property","tool":"t","at":"a..b","property":"p","expected":[]}),
        json!({"kind":"missing_property","tool":"t","at":"","property":"p","expected":[]}),
    ];
    for problem in refused {
        let event = retry(Some(problem.clone()));
        assert!(
            serde_json::from_value::<Event>(event.clone()).is_err(),
            "refused: {problem}"
        );
        assert!(parsed(&event).is_err(), "the frame is refused: {problem}");
        assert!(!schema_accepts(&event)?, "the schema refuses {problem}");
    }
    assert!(ProblemName::new("create_parents").is_some());
    assert!(ProblemName::new("mcp.server-tool_1").is_some());
    assert!(ProblemName::new(&"x".repeat(64)).is_some());
    for name in ["", "a b", "1a", "-a", "a\"", "é"] {
        assert!(ProblemName::new(name).is_none(), "{name:?}");
    }
    // A name the model wrote keeps 32 bytes: a 40-byte identifier-shaped token is dropped, ours keep 64.
    let token = format!("ghp_{}", "A1b2".repeat(9));
    assert_eq!(token.len(), 40);
    assert!(ProblemName::model(&token).is_none());
    assert!(ProblemName::new(&token).is_some());
    assert!(ProblemName::model(&"x".repeat(32)).is_some());
    assert!(ProblemName::model(&"x".repeat(33)).is_none());
    // A place from a JSON pointer.
    for (pointer, place) in [
        ("/edits/0", Some("edits[0]")),
        ("/a/b/12/c", Some("a.b[12].c")),
        ("", None),
        ("/0", None),
        ("/a b", None),
        ("/a/b/c/d/e", None),
        ("/a.b", None),
    ] {
        assert_eq!(
            ProblemPath::from_pointer(pointer)
                .as_ref()
                .map(ProblemPath::as_str),
            place,
            "{pointer:?}"
        );
    }
    Ok(())
}
