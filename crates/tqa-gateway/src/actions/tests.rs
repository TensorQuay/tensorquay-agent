use super::Action;
use axum::http::{HeaderMap, HeaderValue};
use tqa_types::ClientFrame;

#[test]
fn gw_s_021_action_keys_and_bodies_are_exact_and_bounded() -> Result<(), Box<dyn std::error::Error>>
{
    let frame = || ClientFrame::UserMessage {
        lead_brief: None,
        team_wake: None,
        text: "hello".into(),
        attachments: vec![],
        host_keys: Vec::new(),
    };
    let mut headers = HeaderMap::new();
    assert!(Action::new(&headers, "messages".into(), frame()).is_err());
    for key in [String::new(), "contains whitespace".into(), "x".repeat(129)] {
        headers.insert("idempotency-key", HeaderValue::from_str(&key)?);
        assert!(Action::new(&headers, "messages".into(), frame()).is_err());
    }
    headers.insert("idempotency-key", HeaderValue::from_static("request-one"));
    assert!(Action::new(&headers, "messages".into(), frame()).is_ok());
    assert!(
        Action::new(
            &headers,
            "messages".into(),
            ClientFrame::UserMessage {
                lead_brief: None,
                team_wake: None,
                text: String::new(),
                attachments: vec![],
                host_keys: Vec::new(),
            }
        )
        .is_err()
    );
    headers.append("idempotency-key", HeaderValue::from_static("request-two"));
    assert!(Action::new(&headers, "messages".into(), frame()).is_err());
    Ok(())
}
