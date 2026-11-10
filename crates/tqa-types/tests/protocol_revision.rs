//! PROTO-U-008: the protocol revision, its floor, its table and the header and label that carry it.
use serde_json::Value;
use tqa_types::{
    MIN_RUNTIME_REVISION, PROTOCOL_REVISION, PROTOCOL_REVISION_HEADER, PROTOCOL_REVISIONS,
    RUNTIME_REVISION_LABEL, RuntimeTooOld, check_runtime_revision, parse_revision, protocol_schema,
};

#[test]
fn proto_u_008_the_floor_stays_10_under_revision_14() {
    // ADR-0035 raised the floor to 9 (a revision-8 runtime can't read a session.config without the policy). Revision
    // 10 adds the reply_cut error code, which a revision-9 runtime can't read in a history it resumes, so the gateway
    // refuses it as too old (decided on 29 Sep 2026); an upgrade rebuilds every environment anyway. Revision
    // 11 adds host keys only for runtimes announcing it, so the floor stays 10 (guardrails §8, decided on 1 Oct 2026).
    // Revision 12 adds the lead's brief on an agent team member's first turn; only a member needs it, refused at the
    // link below 12, so the floor stays 10 (ADR-0034, decided on 2 Oct 2026). Revision 13 marks the gateway's wake of a
    // team reader; a team's lead or member is refused at the link below 13, and the floor stays 10 (decided on 5 Oct
    // 2026). Revision 14 brings the Desktop; a conversation whose environment declares it is refused at the link
    // below 14, and the floor stays 10 (ADR-0039).
    assert_eq!(MIN_RUNTIME_REVISION, 10);
    assert_eq!(PROTOCOL_REVISION, 14);
    assert_eq!(tqa_types::DESKTOP_REVISION, 14);
    assert_eq!(tqa_types::TEAM_WAKE_REVISION, 13);
    assert_eq!(tqa_types::HOST_KEYS_REVISION, 11);
    assert_eq!(tqa_types::LEAD_BRIEF_REVISION, 12);
    // The revision that brought the decision engine; it doesn't move with later revisions.
    assert_eq!(tqa_types::DECISION_ENGINE_REVISION, 9);
    assert_eq!(PROTOCOL_REVISION_HEADER, "TQA-Protocol-Revision");
    assert_eq!(RUNTIME_REVISION_LABEL, "org.tensorquay.protocol-revision");
}

/// Whether a schema definition, or one of its variants, declares the property or, for a tagged union, a variant
/// whose `type` is that name (a revision may add a whole frame), or, for a closed vocabulary, that value.
fn declares(schema: &Value, definition: &str, name: &str) -> bool {
    let Some(definition) = schema["definitions"].get(definition) else {
        return false;
    };
    let variants = definition["oneOf"].as_array().into_iter().flatten();
    std::iter::once(definition).chain(variants).any(|shape| {
        shape["properties"].get(name).is_some()
            || shape["properties"]["type"]["const"] == name
            || shape["const"] == name
    })
}

#[test]
fn proto_u_008_the_revision_table_matches_the_exported_schema() {
    let schema = protocol_schema();
    assert_eq!(PROTOCOL_REVISIONS.len(), usize::from(PROTOCOL_REVISION));
    for (index, revision) in PROTOCOL_REVISIONS.iter().enumerate() {
        assert_eq!(usize::from(revision.number), index + 1, "{revision:?}");
        assert!(!revision.adds.is_empty(), "{revision:?}");
        assert_eq!(
            revision.schema.is_empty(),
            revision.number == 1,
            "only the base names no field: {revision:?}"
        );
        for &(definition, name) in revision.schema {
            let removed = PROTOCOL_REVISIONS[index..]
                .iter()
                .any(|later| later.removes.contains(&(definition, name)));
            assert_eq!(
                declares(&schema, definition, name),
                !removed,
                "revision {} names {definition}.{name}: the schema has it unless a later revision removed it",
                revision.number
            );
        }
        for &(definition, name) in revision.removes {
            assert!(
                !declares(&schema, definition, name),
                "revision {} removed {definition}.{name}, which the schema still has",
                revision.number
            );
        }
    }
    let named: Vec<_> = PROTOCOL_REVISIONS.iter().map(|r| r.schema).collect();
    assert_eq!(named, expected_schema_names());
    assert!(!declares(&schema, "RouteSnapshot", "no_such_field"));
    assert!(!declares(&schema, "NoSuchDefinition", "push"));
    assert!(!declares(&schema, "ErrorCode", "no_such_code"));
    assert!(
        !declares(&schema, "ClientFrame", "no.such_frame"),
        "a frame name counts only when a variant carries it"
    );
    assert!(
        declares(&schema, "ServerFrame", "workspace.reply"),
        "the reply is exported, though it needs no revision (the gateway is never older than its runtimes)"
    );
}

#[test]
fn proto_u_008_revision_9_removes_the_approval_inputs_and_keeps_the_retired_markers() {
    let schema = protocol_schema();
    let removed: Vec<_> = PROTOCOL_REVISIONS.iter().map(|r| r.removes).collect();
    // Revision 9's row by its number: later revisions add rows after it.
    assert_eq!(
        removed.get(8).copied(),
        Some(
            &[
                ("ClientFrame", "policy"),
                ("WorkspaceSnapshot", "push"),
                ("McpServer", "class"),
                ("ClientFrame", "permission.decision"),
                ("ClientFrame", "permission.forward"),
                ("ClientFrame", "session.park"),
            ][..]
        ),
        "revision 9 removes the approval inputs (ADR-0035); the retired markers stay readable"
    );
    assert!(removed[..8].iter().all(|removes| removes.is_empty()));
    for (definition, name) in [
        ("ServerFrame", "permission.request"),
        ("Event", "permission.resolved"),
        ("Event", "permission.expired"),
        ("Event", "run.parked"),
        ("ErrorCode", "permission_binding"),
    ] {
        assert!(
            declares(&schema, definition, name),
            "the retired {definition}.{name} still parses"
        );
    }
}

#[test]
fn proto_u_008_an_absent_revision_is_one_and_malformed_values_are_refused() {
    assert_eq!(parse_revision(None), Ok(1));
    assert_eq!(parse_revision(Some("4")), Ok(4));
    assert_eq!(parse_revision(Some("65535")), Ok(65_535));
    for malformed in ["", "0", "x", "+4", "-1", " 4", "4 ", "04", "65536", "1.0"] {
        assert!(parse_revision(Some(malformed)).is_err(), "{malformed:?}");
    }
}

#[test]
fn proto_u_008_a_runtime_below_the_floor_is_too_old() {
    assert_eq!(check_runtime_revision(MIN_RUNTIME_REVISION), Ok(()));
    assert_eq!(check_runtime_revision(MIN_RUNTIME_REVISION + 1), Ok(()));
    for has in [1, MIN_RUNTIME_REVISION - 1] {
        let error = check_runtime_revision(has);
        assert_eq!(
            error,
            Err(RuntimeTooOld {
                needs: MIN_RUNTIME_REVISION,
                has
            })
        );
        let text = error.err().map(|e| e.to_string()).unwrap_or_default();
        assert_eq!(
            text,
            format!(
                "runtime protocol revision {has} is older than the required {MIN_RUNTIME_REVISION}"
            )
        );
    }
}

/// What each revision's row names, in order: a raise needs a row here too.
fn expected_schema_names() -> Vec<&'static [(&'static str, &'static str)]> {
    vec![
        &[][..],
        &[("WorkspaceSnapshot", "push")],
        &[("RouteSnapshot", "max_request_bytes")],
        &[("ClientFrame", "fallback_routes")],
        &[("Event", "item")],
        &[("ReasoningSnapshot", "budget_tokens")],
        &[
            ("ClientFrame", "mcp"),
            ("RouteSnapshot", "cache_hint"),
            ("ClientFrame", "workspace.request"),
            ("ClientFrame", "workspace.cancel"),
        ],
        &[
            ("ClientFrame", "profile"),
            ("Environment", "version"),
            ("Event", "version"),
        ],
        &[
            ("ClientFrame", "decision_engine"),
            ("Event", "decision"),
            ("Event", "decision.fallback"),
            ("ErrorCode", "decision_stop"),
            ("ErrorCode", "decision_engine_unavailable"),
            ("ErrorCode", "decision_engine_budget_spent"),
            ("ErrorCode", "approvals_removed"),
            ("ClientFrame", "gateway_revision"),
            ("ClientFrame", "network"),
        ],
        &[
            ("ErrorCode", "reply_cut"),
            ("GitPushFailure", "key_use_unrecorded"),
            ("RouteSnapshot", "first_response_secs"),
        ],
        &[
            ("ClientFrame", "host_keys"),
            ("ClientFrame", "host_ca"),
            ("ClientFrame", "run_host_keys"),
            ("ErrorCode", "host_keys_unsupported"),
            ("ServerFrame", "host_key.used"),
            ("ServerFrame", "host_key.refused"),
        ],
        &[("ClientFrame", "lead_brief"), ("Event", "lead_brief")],
        &[
            ("ClientFrame", "team_wake"),
            ("Event", "team_wake"),
            ("TeamWake", "after"),
        ],
        &[
            ("ClientFrame", "screen.sign_out"),
            ("ServerFrame", "screen.sign_out.done"),
            ("ServerFrame", "screen.reset"),
            ("ServerFrame", "screen.notice"),
            ("ServerFrame", "screen.upload"),
            ("ClientFrame", "screen.stored"),
            ("QueueReason", "desktop_sessions"),
            ("Event", "target"),
            ("ScreenTarget", "role"),
            ("ScreenTarget", "name"),
            ("Event", "evidence"),
            ("ScreenEvidence", "before"),
            ("ScreenEvidence", "after"),
            ("Event", "screen.capture"),
            ("RouteSnapshot", "pixel_grounding"),
            ("ClientFrame", "computer_limits"),
        ],
    ]
}
