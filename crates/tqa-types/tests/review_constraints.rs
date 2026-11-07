//! PROTO-U-007: expiry timestamps and attachment counts are bounded at the wire boundary.

use std::error::Error;

use serde_json::{Value, json};
use tqa_types::{ClientFrame, ErrorCode, MAX_FRAME_BYTES};

fn schema_for(name: &str) -> Result<jsonschema::Validator, Box<dyn Error>> {
    let mut schema: Value = serde_json::from_str(include_str!("../schema/tqa-protocol-v1.json"))?;
    schema["$ref"] = json!(format!("#/definitions/{name}"));
    Ok(jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)?)
}

#[test]
fn proto_u_007_attachment_count_is_capped_at_thirty_two() -> Result<(), Box<dyn Error>> {
    let schema = schema_for("ClientFrame")?;
    for count in [0, 32, 33, 20_000] {
        let attachments: Vec<_> = (0..count).map(|i| format!("uploads/file-{i}")).collect();
        let value = json!({"type":"user.message", "text":"hello", "attachments":attachments});
        let encoded = serde_json::to_vec(&value)?;
        let parsed = ClientFrame::parse(&encoded);
        if count <= 32 {
            assert!(parsed.is_ok());
            assert!(schema.is_valid(&value));
        } else {
            let error = parsed.err().ok_or("accepted too many attachments")?;
            assert_eq!(error.code, ErrorCode::InvalidFrame);
            let field = if encoded.len() > MAX_FRAME_BYTES {
                "$"
            } else {
                "$.attachments"
            };
            assert_eq!(error.path, field);
            assert!(serde_json::from_value::<ClientFrame>(value.clone()).is_err());
            assert!(!schema.is_valid(&value));
        }
    }
    Ok(())
}
