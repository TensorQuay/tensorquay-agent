//! PROTO-U-040: the screenshot upload frames (revision 14, ADR-0039 section 9): `screen.upload` chunks keyed by the
//! PNG's SHA-256, and `screen.stored` with its closed reasons. Each round-trips exactly; a malformed one is refused.
use serde_json::{Value, json};
use tqa_types::{ClientFrame, ErrorCode, ServerFrame, StoreRefusal};

fn id() -> String {
    "ab".repeat(32)
}

fn refused_server(wire: &Value) -> bool {
    matches!(ServerFrame::parse(wire.to_string().as_bytes()), Err(error) if error.code == ErrorCode::InvalidFrame)
}

// TEST: PROTO-U-040
#[test]
fn proto_u_040_an_upload_chunk_round_trips_and_a_malformed_one_is_refused()
-> Result<(), Box<dyn std::error::Error>> {
    let wire = json!({"type": "screen.upload", "id": id(), "chunk": 3, "last": false, "data": "iVBORw0KGgo="});
    let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
    assert_eq!(
        frame,
        ServerFrame::ScreenUpload {
            id: id(),
            chunk: 3,
            last: false,
            data: "iVBORw0KGgo=".to_owned()
        }
    );
    assert_eq!(serde_json::to_value(&frame)?, wire);
    let full = "A".repeat(tqa_types::MAX_UPLOAD_CHUNK_CHARS);
    assert!(
        ServerFrame::parse(
            json!({"type": "screen.upload", "id": id(), "chunk": 0, "last": true, "data": full})
                .to_string()
                .as_bytes()
        )
        .is_ok()
    );
    for wire in [
        json!({"type": "screen.upload", "id": "AB".repeat(32), "chunk": 0, "last": true, "data": "AAAA"}),
        json!({"type": "screen.upload", "id": "ab".repeat(31), "chunk": 0, "last": true, "data": "AAAA"}),
        json!({"type": "screen.upload", "id": id(), "chunk": -1, "last": true, "data": "AAAA"}),
        json!({"type": "screen.upload", "id": id(), "chunk": 0, "last": true, "data": ""}),
        json!({"type": "screen.upload", "id": id(), "chunk": 0, "last": true, "data": "AAA"}),
        json!({"type": "screen.upload", "id": id(), "chunk": 0, "last": true, "data": "AA*A"}),
        json!({"type": "screen.upload", "id": id(), "chunk": 0, "last": true, "data": "A==="}),
        json!({"type": "screen.upload", "id": id(), "chunk": 0, "last": true, "data": "A".repeat(tqa_types::MAX_UPLOAD_CHUNK_CHARS + 4)}),
    ] {
        assert!(refused_server(&wire), "{wire}");
    }
    Ok(())
}

// TEST: PROTO-U-040
#[test]
fn proto_u_040_the_answer_says_kept_or_why_not() -> Result<(), Box<dyn std::error::Error>> {
    let kept = ClientFrame::parse(
        json!({"type": "screen.stored", "id": id(), "ok": true})
            .to_string()
            .as_bytes(),
    )?;
    assert_eq!(
        kept,
        ClientFrame::ScreenStored {
            id: id(),
            ok: true,
            reason: None
        }
    );
    for (wire, reason) in [
        ("too_large", StoreRefusal::TooLarge),
        ("quota_full", StoreRefusal::QuotaFull),
        ("bad_image", StoreRefusal::BadImage),
        ("store_failed", StoreRefusal::StoreFailed),
    ] {
        let value = json!({"type": "screen.stored", "id": id(), "ok": false, "reason": wire});
        let frame = ClientFrame::parse(value.to_string().as_bytes())?;
        assert_eq!(
            frame,
            ClientFrame::ScreenStored {
                id: id(),
                ok: false,
                reason: Some(reason)
            }
        );
        assert_eq!(serde_json::to_value(&frame)?, value);
    }
    for wire in [
        json!({"type": "screen.stored", "id": id(), "ok": false}),
        json!({"type": "screen.stored", "id": id(), "ok": true, "reason": "too_large"}),
        json!({"type": "screen.stored", "id": id(), "ok": false, "reason": "disk_full"}),
        json!({"type": "screen.stored", "id": "x", "ok": true}),
    ] {
        assert!(
            ClientFrame::parse(wire.to_string().as_bytes()).is_err(),
            "{wire}"
        );
    }
    Ok(())
}
