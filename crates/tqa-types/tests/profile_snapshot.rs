//! PROTO-U-018: the revision-8 profile snapshot in `session.config` and the environment version (ADR-0012 decision 6,
//! ADR-0032 decision 7). Canonical forms round trip exactly; every bound is refused on parse and, where JSON Schema can
//! say it, by the exported schema alike; a runtime below revision 8 is refused at the link, so the gateway never sends it
//! either field.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    ClientFrame, Environment, ErrorCode, Event, MAX_PROFILE_INSTRUCTIONS_BYTES, MAX_PROFILE_SKILLS,
    MAX_PROFILE_TOOLS, PROFILE_NAME_PATTERN, ProfileSnapshot, RuntimeTooOld, SKILL_NAME_PATTERN,
    ServerFrame, SkillsFilter, check_runtime_revision, protocol_schema,
};

type TestResult = Result<(), Box<dyn Error>>;

fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}
fn config(profile: Option<Value>, environment: Option<Value>) -> Value {
    let mut frame = json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":{"name":"local","protocol":"openai_chat","model":"m","relay_path":"/relay/r/local",
            "context_window":8192,"max_output":1024,"native_tools":true,"parallel_tools":true,"vision":false,
            "reasoning":null},
        "network":{"allowed":[],"more":0},
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    });
    if let Some(value) = profile {
        frame["profile"] = value;
    }
    if let Some(value) = environment {
        frame["environment"] = value;
    }
    frame
}
fn profile(edit: impl FnOnce(&mut Value)) -> Value {
    let mut value = json!({"name":"reviewer","instructions":"Review; never edit.","tools":["read_file","grep"],"skills":"none"});
    edit(&mut value);
    config(Some(value), None)
}
fn environment(version: &Value) -> Value {
    config(
        None,
        Some(
            json!({"template":"workstation","digest":digest(),"capabilities":["shell"],"version":version}),
        ),
    )
}
fn schema_accepts(value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}
fn parse(value: &Value) -> Result<ClientFrame, tqa_types::FrameError> {
    ClientFrame::parse(&serde_json::to_vec(value).unwrap_or_default())
}
fn round_trips(frame: &Value) -> TestResult {
    let parsed = parse(frame).map_err(|error| format!("{error:?}: {frame}"))?;
    assert_eq!(&serde_json::to_value(&parsed)?, frame);
    assert!(schema_accepts(frame)?, "the schema refuses {frame}");
    Ok(())
}
/// The parser refuses the frame under `path`; the schema refuses it too unless the bound is one JSON Schema cannot say.
fn refused(frame: &Value, path: &str, schema_too: bool) -> TestResult {
    let error = parse(frame)
        .err()
        .ok_or_else(|| format!("accepted: {frame}"))?;
    assert_eq!(error.code, ErrorCode::InvalidFrame, "{frame}");
    assert!(
        error.path.starts_with(path),
        "{} is not under {path}: {frame}",
        error.path
    );
    if schema_too {
        assert!(!schema_accepts(frame)?, "the schema accepts {frame}");
    }
    Ok(())
}

#[test]
fn proto_u_018_the_agreed_wire_forms_round_trip_exactly() -> TestResult {
    round_trips(&profile(|_| {}))?;
    round_trips(&profile(|p| p["skills"] = json!("all")))?;
    round_trips(&profile(|p| {
        p["skills"] = json!(["code-review", "release-notes"]);
    }))?;
    round_trips(&profile(|p| {
        p.as_object_mut().map(|o| o.remove("instructions"));
        p.as_object_mut().map(|o| o.remove("tools"));
    }))?;
    round_trips(&profile(|p| p["tools"] = json!([])))?;
    round_trips(&environment(&json!(3)))?;
    round_trips(&environment(&json!(u64::MAX)))?;
    round_trips(&config(None, None))?;
    Ok(())
}

#[test]
fn proto_u_018_the_snapshot_types_hold_the_agreed_values() -> TestResult {
    let ClientFrame::SessionConfig {
        profile: Some(snapshot),
        ..
    } = parse(&profile(|p| p["skills"] = json!(["code-review"])))?
    else {
        return Err("no profile".into());
    };
    assert_eq!(
        snapshot,
        ProfileSnapshot {
            name: "reviewer".into(),
            instructions: Some("Review; never edit.".into()),
            tools: Some(vec!["read_file".into(), "grep".into()]),
            skills: SkillsFilter::Only(vec!["code-review".into()]),
        }
    );
    // An absent filter is every skill, and it is written out.
    let bare = profile(|p| {
        p.as_object_mut().map(|o| o.remove("skills"));
    });
    let parsed = parse(&bare)?;
    let ClientFrame::SessionConfig {
        profile: Some(snapshot),
        ..
    } = &parsed
    else {
        return Err("no profile".into());
    };
    assert_eq!(snapshot.skills, SkillsFilter::All);
    assert_eq!(
        serde_json::to_value(&parsed)?["profile"]["skills"],
        json!("all")
    );
    // Without a profile or a version, nothing new is written.
    let plain = serde_json::to_value(parse(&config(None, None))?)?;
    assert!(plain.get("profile").is_none());
    let ClientFrame::SessionConfig { environment, .. } = parse(&environment(&json!(7)))? else {
        return Err("not a session.config".into());
    };
    assert_eq!(environment.version, Some(7));
    Ok(())
}

#[test]
fn proto_u_018_the_name_follows_its_rule() -> TestResult {
    assert_eq!(PROFILE_NAME_PATTERN, "^[a-z][a-z0-9_-]{0,63}$");
    for name in ["a", "code_review-2", &format!("a{}", "b".repeat(63))] {
        round_trips(&profile(|p| p["name"] = json!(name)))?;
    }
    for name in [
        "",
        "Reviewer",
        "2fast",
        "-a",
        "a.b",
        "a b",
        &format!("a{}", "b".repeat(64)),
    ] {
        refused(&profile(|p| p["name"] = json!(name)), "$.profile", true)?;
    }
    Ok(())
}

#[test]
fn proto_u_018_instructions_are_bounded_in_bytes_and_never_empty_or_nul() -> TestResult {
    assert_eq!(MAX_PROFILE_INSTRUCTIONS_BYTES, 16 * 1024);
    let longest = "a".repeat(MAX_PROFILE_INSTRUCTIONS_BYTES);
    round_trips(&profile(|p| p["instructions"] = json!(longest)))?;
    refused(
        &profile(|p| p["instructions"] = json!("a".repeat(MAX_PROFILE_INSTRUCTIONS_BYTES + 1))),
        "$.profile",
        true,
    )?;
    // The bound is bytes: 8193 two-byte characters are 16386 bytes. JSON Schema counts characters, so only the parser
    // can refuse this one.
    refused(
        &profile(|p| p["instructions"] = json!("é".repeat(8193))),
        "$.profile",
        false,
    )?;
    refused(
        &profile(|p| p["instructions"] = json!("")),
        "$.profile",
        true,
    )?;
    refused(
        &profile(|p| p["instructions"] = json!("a\u{0}b")),
        "$.profile",
        true,
    )?;
    refused(
        &profile(|p| p["instructions"] = json!(null)),
        "$.profile",
        true,
    )?;
    Ok(())
}

#[test]
fn proto_u_018_tools_are_distinct_built_in_names() -> TestResult {
    assert_eq!(MAX_PROFILE_TOOLS, 64);
    let most: Vec<String> = (0..MAX_PROFILE_TOOLS)
        .map(|i| format!("tool_{i}"))
        .collect();
    round_trips(&profile(|p| p["tools"] = json!(most)))?;
    let too_many: Vec<String> = (0..=MAX_PROFILE_TOOLS)
        .map(|i| format!("tool_{i}"))
        .collect();
    refused(
        &profile(|p| p["tools"] = json!(too_many)),
        "$.profile",
        true,
    )?;
    refused(
        &profile(|p| p["tools"] = json!(["grep", "grep"])),
        "$.profile",
        true,
    )?;
    for name in [
        "mcp.tracker.create_issue",
        "Read",
        "",
        "read-file",
        &"a".repeat(65),
    ] {
        refused(&profile(|p| p["tools"] = json!([name])), "$.profile", true)?;
    }
    refused(&profile(|p| p["tools"] = json!("grep")), "$.profile", true)?;
    Ok(())
}

#[test]
fn proto_u_018_skills_are_all_none_or_a_bounded_list() -> TestResult {
    assert_eq!(MAX_PROFILE_SKILLS, 64);
    assert_eq!(SKILL_NAME_PATTERN, "^[a-z0-9-]{1,64}$");
    let most: Vec<String> = (0..MAX_PROFILE_SKILLS)
        .map(|i| format!("skill-{i}"))
        .collect();
    round_trips(&profile(|p| p["skills"] = json!(most)))?;
    round_trips(&profile(|p| p["skills"] = json!(["9", &"a".repeat(64)])))?;
    let too_many: Vec<String> = (0..=MAX_PROFILE_SKILLS)
        .map(|i| format!("skill-{i}"))
        .collect();
    for skills in [
        json!(too_many),
        json!([]),
        json!(["review", "review"]),
        json!(["Review"]),
        json!(["a_b"]),
        json!([""]),
        json!([&"a".repeat(65)]),
        json!("some"),
        json!("All"),
        json!(null),
        json!({"only": ["review"]}),
    ] {
        refused(
            &profile(|p| p["skills"] = skills.clone()),
            "$.profile",
            true,
        )?;
    }
    Ok(())
}

#[test]
fn proto_u_018_unknown_or_missing_profile_fields_are_refused() -> TestResult {
    refused(&profile(|p| p["model"] = json!("local")), "$.profile", true)?;
    refused(
        &profile(|p| {
            p.as_object_mut().map(|o| o.remove("name"));
        }),
        "$.profile",
        true,
    )?;
    refused(&config(Some(json!("reviewer")), None), "$.profile", true)?;
    refused(&config(Some(json!(null)), None), "$.profile", true)?;
    Ok(())
}

#[test]
fn proto_u_018_the_environment_version_is_positive_and_needs_a_pinned_image() -> TestResult {
    for version in [json!(0), json!(-1), json!(1.5), json!("3"), json!(null)] {
        refused(&environment(&version), "$", true)?;
    }
    // The legacy Minimal default has no image, so it cannot name a version of one.
    let legacy = config(
        None,
        Some(json!({"template":"minimal","digest":"","capabilities":["shell"],"version":1})),
    );
    refused(&legacy, "$", true)?;
    let pinned = Environment::new("workstation".into(), digest(), vec![])?;
    assert_eq!(pinned.version, None);
    assert_eq!(pinned.clone().with_version(Some(4))?.version, Some(4));
    assert!(pinned.clone().with_version(Some(0)).is_err());
    assert!(Environment::default().with_version(Some(1)).is_err());
    assert!(Environment::default().with_version(None)?.is_legacy());
    Ok(())
}

#[test]
fn proto_u_018_environment_changed_may_carry_the_version() -> TestResult {
    let value = json!({"type":"environment.changed","template":"custom","capabilities":["shell"],"version":2});
    let event: Event = serde_json::from_value(value.clone())?;
    assert_eq!(serde_json::to_value(&event)?, value);
    let Event::EnvironmentChanged { version, .. } = event else {
        return Err("not environment.changed".into());
    };
    assert_eq!(version, Some(2));
    let without =
        json!({"type":"environment.changed","template":"custom","capabilities":["shell"]});
    let event: Event = serde_json::from_value(without.clone())?;
    assert_eq!(
        serde_json::to_value(&event)?,
        without,
        "an absent version stays absent"
    );
    for version in [json!(0), json!(-2), json!("2"), json!(null)] {
        let frame = json!({"type":"event","seq":1,"run_id":null,"step":null,"ts":"2000-01-01T00:00:00Z",
            "event":{"type":"environment.changed","template":"custom","capabilities":["shell"],"version":version}});
        let error = ServerFrame::parse(&serde_json::to_vec(&frame)?)
            .err()
            .ok_or_else(|| format!("accepted version {version}"))?;
        assert_eq!(error.code, ErrorCode::InvalidFrame);
    }
    // The revision table's check is by property name, and another event already has a version, so check this variant.
    let schema = protocol_schema();
    let changed = schema["definitions"]["Event"]["oneOf"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|shape| shape["properties"]["type"]["const"] == "environment.changed")
        .ok_or("no environment.changed variant")?;
    assert_eq!(changed["properties"]["version"]["minimum"], json!(1));
    Ok(())
}

#[test]
fn proto_u_018_a_runtime_below_revision_8_is_refused_so_it_never_receives_either_field() {
    // The floor is now 10 (reply_cut, after ADR-0035's 9), so revision 7 and 8 runtimes, and 9, are all refused.
    assert_eq!(check_runtime_revision(10), Ok(()));
    for has in [7, 8, 9] {
        assert_eq!(
            check_runtime_revision(has),
            Err(RuntimeTooOld { needs: 10, has })
        );
    }
}
