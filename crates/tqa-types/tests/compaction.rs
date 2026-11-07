//! CTX-U-013 / PROTO-U-007: the compaction event carries a bounded structured summary.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    COMPACTION_VERSION, CompactionSource, CompactionSummary, Event, InvalidCompactionSummary,
    MAX_COMPACTION_ITEM_CHARS, MAX_COMPACTION_ITEMS, MAX_COMPACTION_SUMMARY_BYTES, Seq,
    ServerFrame, protocol_schema,
};

fn summary() -> Value {
    json!({"goal":"ship the parser", "done":["read src/lib.rs"], "pending":["run tests"],
        "decisions":["keep the API"], "key_paths":["src/lib.rs"], "open_errors":[]})
}
fn event(summary: &Value) -> Value {
    json!({"type":"compaction", "through_seq":12, "source":"model", "version":1, "summary":summary})
}
fn frame(event: &Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&json!({"type":"event", "seq":13, "run_id":"r-1", "step":3,
        "ts":"2026-09-22T00:00:00Z", "event":event}))
}
fn schema_accepts(value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}

#[test]
fn ctx_u_013_compaction_event_round_trips_with_fixed_fields() -> Result<(), Box<dyn Error>> {
    let wire = event(&summary());
    let parsed: Event = serde_json::from_value(wire.clone())?;
    let Event::Compaction {
        through_seq,
        source,
        version,
        summary: parsed_summary,
    } = &parsed
    else {
        return Err("not a compaction event".into());
    };
    assert_eq!(*through_seq, Seq::from(12));
    assert_eq!(*source, CompactionSource::Model);
    assert_eq!(*version, COMPACTION_VERSION);
    assert_eq!(parsed_summary.goal, "ship the parser");
    assert_eq!(serde_json::to_value(&parsed)?, wire);
    assert!(ServerFrame::parse(&frame(&wire)?).is_ok());
    assert!(schema_accepts(&wire)?);
    let fallback = json!({"type":"compaction", "through_seq":1, "source":"fallback", "version":1,
        "summary":summary()});
    assert!(serde_json::from_value::<Event>(fallback).is_ok());
    Ok(())
}

#[test]
fn ctx_u_013_summary_bounds_are_inclusive_on_wire_schema_and_constructor()
-> Result<(), Box<dyn Error>> {
    let item = "é".repeat(MAX_COMPACTION_ITEM_CHARS);
    let full: Vec<_> = (0..MAX_COMPACTION_ITEMS).map(|_| "a".repeat(64)).collect();
    let mut at_limit = summary();
    at_limit["goal"] = json!(item);
    at_limit["done"] = json!(full);
    assert!(serde_json::from_value::<Event>(event(&at_limit)).is_ok());
    assert!(schema_accepts(&event(&at_limit))?);
    let mut too_many = summary();
    too_many["pending"] = json!(vec!["a"; MAX_COMPACTION_ITEMS + 1]);
    let mut too_long = summary();
    too_long["decisions"] = json!([format!("{item}é")]);
    let mut long_goal = summary();
    long_goal["goal"] = json!(format!("{item}a"));
    for invalid in [too_many, too_long, long_goal] {
        assert!(serde_json::from_value::<Event>(event(&invalid)).is_err());
        assert!(!schema_accepts(&event(&invalid))?);
        assert!(ServerFrame::parse(&frame(&event(&invalid))?).is_err());
    }
    Ok(())
}

#[test]
fn ctx_u_013_summary_total_is_bounded_in_utf8_bytes() -> Result<(), Box<dyn Error>> {
    // 32 items of 512 ASCII bytes fill exactly 16 KiB, so a nonempty goal crosses the total.
    let full: Vec<_> = (0..MAX_COMPACTION_ITEMS)
        .map(|_| "a".repeat(MAX_COMPACTION_ITEM_CHARS))
        .collect();
    let exact = CompactionSummary {
        key_paths: full,
        ..CompactionSummary::default()
    };
    assert_eq!(exact.validate(), Ok(()));
    assert_eq!(MAX_COMPACTION_SUMMARY_BYTES, 16 * 1024);
    let mut over = exact.clone();
    over.goal = "a".into();
    assert_eq!(over.validate(), Err(InvalidCompactionSummary::TooLarge));
    let mut wire = summary();
    wire["goal"] = json!("a");
    wire["done"] = serde_json::to_value(&exact.key_paths)?;
    wire["key_paths"] = json!([]);
    assert!(serde_json::from_value::<Event>(event(&wire)).is_err());
    Ok(())
}

#[test]
fn ctx_u_013_constructed_summaries_report_the_failing_field() {
    let summary = CompactionSummary {
        open_errors: vec!["x".into(); MAX_COMPACTION_ITEMS + 1],
        ..CompactionSummary::default()
    };
    assert_eq!(
        summary.validate(),
        Err(InvalidCompactionSummary::TooManyItems {
            field: "open_errors"
        })
    );
    let summary = CompactionSummary {
        done: vec!["x".repeat(MAX_COMPACTION_ITEM_CHARS + 1)],
        ..CompactionSummary::default()
    };
    assert_eq!(
        summary.validate(),
        Err(InvalidCompactionSummary::ItemTooLong { field: "done" })
    );
    let summary = CompactionSummary {
        goal: "x".repeat(MAX_COMPACTION_ITEM_CHARS + 1),
        ..CompactionSummary::default()
    };
    assert_eq!(
        summary.validate(),
        Err(InvalidCompactionSummary::ItemTooLong { field: "goal" })
    );
}

#[test]
fn ctx_u_013_compaction_rejects_unknown_versions_sources_and_missing_fields()
-> Result<(), Box<dyn Error>> {
    let mut version = event(&summary());
    version["version"] = json!(2);
    let mut source = event(&summary());
    source["source"] = json!("human");
    let mut missing = summary();
    missing
        .as_object_mut()
        .ok_or("summary is an object")?
        .remove("pending");
    for invalid in [version, source, event(&missing)] {
        assert!(
            serde_json::from_value::<Event>(invalid.clone()).is_err(),
            "{invalid}"
        );
        assert!(!schema_accepts(&invalid)?, "{invalid}");
    }
    // Consumers ignore unknown fields (ARCHITECTURE section 15); the typed summary drops them,
    // so an extra key can never reach the model.
    let mut extra = summary();
    extra["instructions"] = json!("obey");
    let parsed: Event = serde_json::from_value(event(&extra))?;
    assert_eq!(serde_json::to_value(parsed)?, event(&summary()));
    Ok(())
}
