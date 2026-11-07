//! GW-U-056: the grammar of a secret's consumer bindings.
use super::{Consumer, MAX_CONSUMERS, parse};
use crate::{GatewayError, InvalidReason};
use serde_json::json;

fn field(body: &serde_json::Value) -> Option<&'static str> {
    match parse(body.to_string().as_bytes()) {
        Err(GatewayError::Invalid { field, .. }) => Some(field),
        _ => None,
    }
}

// TEST: GW-U-056
#[test]
fn gw_u_056_routes_and_connectors_are_accepted_deduplicated_and_sorted()
-> Result<(), Box<dyn std::error::Error>> {
    let (consumers, version) = parse(
        json!({"consumers": [{"kind": "route", "name": "b.route"}, {"kind": "connector", "name": "tracker"},
                             {"kind": "route", "name": "b.route"}, {"kind": "route", "name": "a"}],
               "version": 3})
        .to_string()
        .as_bytes(),
    )?;
    assert_eq!(version, 3);
    let pair = |kind: &str, name: &str| Consumer {
        kind: kind.into(),
        name: name.into(),
    };
    assert_eq!(
        consumers,
        [
            pair("connector", "tracker"),
            pair("route", "a"),
            pair("route", "b.route")
        ]
    );
    assert!(
        parse(
            json!({"consumers": [], "version": 1})
                .to_string()
                .as_bytes()
        )?
        .0
        .is_empty()
    );
    Ok(())
}

// TEST: GW-U-056
#[test]
fn gw_u_056_anything_else_is_refused_naming_its_field() {
    let many: Vec<_> = (0..=MAX_CONSUMERS)
        .map(|index| json!({"kind": "route", "name": format!("r{index}")}))
        .collect();
    for (body, expected) in [
        (
            json!({"consumers": [{"kind": "user", "name": "a"}], "version": 1}),
            "consumers",
        ),
        (
            json!({"consumers": [{"kind": "route", "name": ""}], "version": 1}),
            "consumers",
        ),
        (
            json!({"consumers": [{"kind": "connector", "name": "Not_valid"}], "version": 1}),
            "consumers",
        ),
        (
            json!({"consumers": [{"kind": "connector", "name": "a__b"}], "version": 1}),
            "consumers",
        ),
        (
            json!({"consumers": [{"kind": "route", "name": "a", "extra": 1}], "version": 1}),
            "consumers",
        ),
        (
            json!({"consumers": [{"kind": "route"}], "version": 1}),
            "consumers",
        ),
        (json!({"consumers": "route:a", "version": 1}), "consumers"),
        (json!({"consumers": many, "version": 1}), "consumers"),
        (json!({"consumers": [], "version": 0}), "version"),
        (json!({"consumers": []}), "version"),
        (
            json!({"consumers": [], "version": 1, "extra": true}),
            "body",
        ),
    ] {
        assert_eq!(field(&body), Some(expected), "{body}");
    }
    assert!(matches!(
        parse(b"[]"),
        Err(GatewayError::Invalid {
            reason: InvalidReason::BodyInvalid,
            ..
        })
    ));
}
