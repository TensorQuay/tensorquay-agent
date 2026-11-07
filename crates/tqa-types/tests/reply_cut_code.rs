//! PROTO-U-029: `reply_cut`, the failure of a reply the model ended at its output limit while the run's budget was
//! left (CORE-U-066). It is its own stable code on the wire and in the schema, added by protocol revision 10.
use serde_json::json;
use tqa_types::{ErrorCode, PROTOCOL_REVISIONS, protocol_schema};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn proto_u_029_reply_cut_is_its_own_code_on_the_wire() -> TestResult {
    assert_eq!(ErrorCode::ReplyCut.as_str(), "reply_cut");
    assert_eq!(
        serde_json::to_value(ErrorCode::ReplyCut)?,
        json!("reply_cut")
    );
    assert_eq!(
        serde_json::from_value::<ErrorCode>(json!("reply_cut"))?,
        ErrorCode::ReplyCut
    );
    assert_ne!(ErrorCode::ReplyCut, ErrorCode::Budget);
    let schema = protocol_schema();
    let codes = schema["definitions"]["ErrorCode"].to_string();
    assert!(codes.contains("\"reply_cut\""), "{codes}");
    Ok(())
}

#[test]
fn proto_u_029_revision_10_adds_reply_cut() -> TestResult {
    let row = PROTOCOL_REVISIONS.get(9).ok_or("no revision 10")?;
    assert_eq!(row.number, 10);
    // Revision 10 also gained the Git push's key_use_unrecorded in place, before it reached main.
    assert!(row.schema.contains(&("ErrorCode", "reply_cut")), "{row:?}");
    assert!(row.removes.is_empty());
    Ok(())
}
