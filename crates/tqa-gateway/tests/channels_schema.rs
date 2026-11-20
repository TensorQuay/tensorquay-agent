//! The human API's document keeps up with the channel types (review of 20 Nov 2026): `openapi.json` is written by hand
//! and the page validates every message against it, so a notice kind or field the gateway sends but the document
//! lacks stops a channel's log from loading. G-08 compares only the runtime protocol's exported schema, not this
//! document (GW-U-235).
use serde_json::Value;
use tqa_types::{ChannelNoticeKind, MessageBody};

/// Every notice kind: the match is exhaustive, so a new kind doesn't compile until it is listed here.
fn every_kind() -> Vec<ChannelNoticeKind> {
    use ChannelNoticeKind as K;
    let all = vec![
        K::WakeBudget80,
        K::WakeBudgetSpent,
        K::WakesBounded,
        K::UnreachableRuntime,
        K::MemberDeleted,
        K::WakeBudgetSet,
        K::WokenRunFailed,
        K::WokenRunStopped,
    ];
    for kind in &all {
        match kind {
            K::WakeBudget80
            | K::WakeBudgetSpent
            | K::WakesBounded
            | K::UnreachableRuntime
            | K::MemberDeleted
            | K::WakeBudgetSet
            | K::WokenRunFailed
            | K::WokenRunStopped => {}
        }
    }
    all
}

fn notice_schema() -> Result<Value, Box<dyn std::error::Error>> {
    let document: Value = serde_json::from_str(include_str!("../openapi.json"))?;
    document["components"]["schemas"]["ChannelMessageBody"]["oneOf"]
        .as_array()
        .ok_or("oneOf")?
        .iter()
        .find(|body| body["properties"]["type"]["const"] == "notice")
        .cloned()
        .ok_or_else(|| "no notice body in openapi.json".into())
}

// TEST: GW-U-235
#[test]
fn gw_u_235_every_notice_kind_and_field_is_in_the_api_document()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = notice_schema()?;
    let listed: Vec<&str> = schema["properties"]["kind"]["enum"]
        .as_array()
        .ok_or("kind enum")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for kind in every_kind() {
        let name = serde_json::to_value(kind)?;
        let name = name.as_str().ok_or("kind name")?;
        assert!(
            listed.contains(&name),
            "openapi.json's notice kinds lack {name}: {listed:?}"
        );
    }
    // Every field a notice carries, filled, is a property the document allows (it allows no others).
    let notice = MessageBody::Notice {
        kind: ChannelNoticeKind::WakeBudgetSet,
        member: Some("Ana".into()),
        member_id: Some("01a1f9c4-dba5-75a5-829d-7447f02597c7".into()),
        code: Some("budget".into()),
        refusal: Some("gateway.channel_budget 409".into()),
        resets_at: Some("2026-11-22T03:00:00Z".into()),
        text: "Ana changed this channel's budget.".into(),
    };
    let MessageBody::Notice { .. } = &notice else {
        return Err("not a notice".into());
    };
    let sent = serde_json::to_value(&notice)?;
    for field in sent.as_object().ok_or("object")?.keys() {
        assert!(
            schema["properties"].get(field).is_some(),
            "openapi.json's notice lacks the field {field}"
        );
    }
    Ok(())
}
