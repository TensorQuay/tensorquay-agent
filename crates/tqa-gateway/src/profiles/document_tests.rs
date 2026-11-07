//! GW-U-049 (ADR-0012): a profile's fields are validated with field-level reasons, the same from the file and the API.
use serde_json::{Value, json};

use super::{ProfileInput, validate};
use crate::InvalidReason;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn full() -> Value {
    json!({
        "name": "reviewer",
        "display_name": "Code reviewer",
        "description": "Reads, never writes.",
        "instructions": "Review the change; do not edit files.",
        "model_route": "local",
        "template": "workstation",
        "tools": ["read_file", "grep", "glob"],
        "skills": ["code-review"],
        "mcp_servers": ["docs"],
        "connectors": ["tracker"],
        "limits": {"max_steps": 20, "wall_clock_secs": 600},
        "busy_policy": "refuse",
    })
}

fn checked(value: Value) -> std::result::Result<super::Profile, (&'static str, InvalidReason)> {
    let input: ProfileInput =
        serde_json::from_value(value).map_err(|_| ("body", InvalidReason::BodyInvalid))?;
    validate(input)
}

#[test]
fn gw_u_049_a_complete_profile_is_accepted_as_written() -> Result<()> {
    let profile = checked(full()).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(
        (profile.name.as_str(), profile.display_name.as_str()),
        ("reviewer", "Code reviewer")
    );
    assert_eq!(
        profile.tools.as_deref(),
        Some(&["read_file".to_owned(), "grep".to_owned(), "glob".to_owned()][..])
    );
    assert_eq!(profile.limits.max_steps, Some(20));
    // Only the name and display name are required; everything else narrows nothing when left out.
    let minimal = checked(json!({"name": "plain", "display_name": "Plain"}))
        .map_err(|refusal| format!("{refusal:?}"))?;
    assert!(minimal.tools.is_none() && minimal.connectors.is_empty());
    assert_eq!(minimal.skills, super::Skills::All);
    // `none` turns every skill off.
    let mut none = full();
    none["skills"] = json!("none");
    assert_eq!(
        checked(none)
            .map_err(|refusal| format!("{refusal:?}"))?
            .skills,
        super::Skills::None
    );
    // No built-in tools at all is a profile that works through connectors only.
    let mut connectors_only = full();
    connectors_only["tools"] = json!([]);
    let connectors_only = checked(connectors_only).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(connectors_only.tools, Some(Vec::new()));
    Ok(())
}

/// The refusal for `full()` with `field` set to `value`.
fn refused(field: &str, value: Value) -> Option<(&'static str, InvalidReason)> {
    let mut profile = full();
    profile[field] = value;
    checked(profile).err()
}

fn many(count: usize, prefix: &str) -> Value {
    json!(
        (0..count)
            .map(|index| format!("{prefix}{index}"))
            .collect::<Vec<_>>()
    )
}

#[test]
fn gw_u_049_each_text_field_has_its_reason() {
    use InvalidReason::{
        DisplayNameInvalid, NameInvalid, RouteNameInvalid, ValueEmpty, ValueNul, ValueTooLong,
    };
    let cases = [
        ("name", json!("Reviewer"), "name", NameInvalid),
        (
            "display_name",
            json!("   "),
            "display_name",
            DisplayNameInvalid,
        ),
        (
            "display_name",
            json!("x".repeat(65)),
            "display_name",
            DisplayNameInvalid,
        ),
        (
            "description",
            json!("x".repeat(513)),
            "description",
            ValueTooLong,
        ),
        ("description", json!("a\u{0}b"), "description", ValueNul),
        (
            "instructions",
            json!("x".repeat(16 * 1024 + 1)),
            "instructions",
            ValueTooLong,
        ),
        ("instructions", json!("a\u{0}b"), "instructions", ValueNul),
        // Empty instructions are left out, never sent empty.
        ("instructions", json!(""), "instructions", ValueEmpty),
        ("model_route", json!(""), "model_route", RouteNameInvalid),
        (
            "model_route",
            json!("a\nb"),
            "model_route",
            RouteNameInvalid,
        ),
        ("template", json!("Work Station"), "template", NameInvalid),
    ];
    for (set, value, field, reason) in cases {
        assert_eq!(refused(set, value), Some((field, reason)), "{set}");
    }
}

#[test]
fn gw_u_049_each_list_has_its_reason() {
    use InvalidReason::{ListInvalid, UnknownReference};
    let cases = [
        ("tools", json!(["grep", "grep"]), ListInvalid),
        ("tools", json!(["teleport"]), UnknownReference),
        ("skills", json!(["Code Review"]), ListInvalid),
        ("skills", many(65, "s"), ListInvalid),
        // "none" is the one spelling of no skills.
        ("skills", json!([]), ListInvalid),
        ("mcp_servers", many(9, "m"), ListInvalid),
        ("mcp_servers", json!(["docs", "docs"]), ListInvalid),
        ("connectors", many(65, "c"), ListInvalid),
        ("connectors", json!([""]), ListInvalid),
        // A conversation holds at most eight servers and connectors together (G6), so a profile does too.
        (
            "connectors",
            json!(["c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8"]),
            ListInvalid,
        ),
    ];
    for (field, value, reason) in cases {
        assert_eq!(refused(field, value), Some((field, reason)), "{field}");
    }
}

#[test]
fn gw_u_049_limits_and_shapes_have_their_reasons() {
    use InvalidReason::LimitOutOfRange;
    let limits = [
        (json!({"max_steps": 0}), "limits.max_steps"),
        (json!({"wall_clock_secs": 86_401}), "limits.wall_clock_secs"),
        (
            json!({"tool_result_bytes": 1023}),
            "limits.tool_result_bytes",
        ),
        (json!({"max_output_tokens": 0}), "limits.max_output_tokens"),
    ];
    for (value, field) in limits {
        assert_eq!(
            refused("limits", value),
            Some((field, LimitOutOfRange)),
            "{field}"
        );
    }
    // Shapes serde refuses: unknown fields (the policy and rules ADR-0035 removed among them), a skills word other than
    // all or none.
    let body = Some(("body", InvalidReason::BodyInvalid));
    assert_eq!(refused("colour", json!("red")), body);
    assert_eq!(refused("policy", json!("read_only")), body);
    assert_eq!(refused("rules", json!([])), body);
    assert_eq!(refused("skills", json!("some")), body);
    assert_eq!(refused("skills", json!(5)), body);
    // Names may carry digits, underscores and dashes after their first letter; "all" may be spelled out.
    assert_eq!(refused("name", json!("review_2-x")), None);
    assert_eq!(refused("skills", json!("all")), None);
}

// TEST: GW-U-066
#[test]
fn gw_u_066_a_profile_may_list_the_channel_tools() -> Result<()> {
    let mut profile = full();
    profile["tools"] = json!([
        "read_file",
        "channel_post",
        "channel_read",
        "channel_wait",
        "task_claim"
    ]);
    let accepted = checked(profile).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(accepted.tools.as_ref().map(Vec::len), Some(5));
    // Only the four names, exactly.
    for name in ["channel_postx", "channel", "task_claims", "Channel_post"] {
        assert_eq!(
            refused("tools", json!([name])),
            Some(("tools", InvalidReason::UnknownReference)),
            "{name}"
        );
    }
    Ok(())
}

// TEST: GW-U-083
#[test]
fn gw_u_083_repositories_is_removed_and_git_push_is_not_a_tool_choice() -> Result<()> {
    // Removed with git_push (ADR-0037 §7): any value is refused by name, never ignored.
    for value in [json!(true), json!(false), json!("yes")] {
        assert_eq!(
            refused("repositories", value.clone()),
            Some(("repositories", InvalidReason::Removed)),
            "{value}"
        );
    }
    assert_eq!(
        refused("tools", json!(["read_file", "git_push"])),
        Some(("tools", InvalidReason::UnknownReference))
    );
    // A copy an earlier release stored with the flag reads without it, and writes back without it.
    let stored: super::Profile = serde_json::from_value(json!({
        "name": "old", "display_name": "Old", "skills": "all", "mcp_servers": [], "connectors": [], "limits": {},
        "repositories": true,
    }))?;
    assert_eq!(serde_json::to_value(&stored)?.get("repositories"), None);
    Ok(())
}
