//! GW-U-050: a profile's limits merged with the install's are each the smaller one, and the runtime gets the profile's
//! own part as its snapshot.
use serde_json::{Value, json};
use tqa_types::RunLimits;

use super::{Limits, Profile, limits, snapshot};
use crate::GatewayError;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn profile() -> Result<Profile> {
    Ok(serde_json::from_value(json!({
        "name": "p", "display_name": "P", "skills": "all", "mcp_servers": [], "connectors": [], "limits": {},
    }))?)
}

#[test]
fn gw_u_050_each_limit_is_the_smaller() {
    let install = RunLimits {
        max_steps: 100,
        wall_clock_secs: 1800,
        tool_result_bytes: 8192,
        max_output_tokens: 131_072,
    };
    assert_eq!(limits(install, &Limits::default()), install);
    let lower = Limits {
        max_steps: Some(10),
        wall_clock_secs: Some(60),
        tool_result_bytes: Some(2048),
        max_output_tokens: Some(1000),
    };
    assert_eq!(
        limits(install, &lower),
        RunLimits {
            max_steps: 10,
            wall_clock_secs: 60,
            tool_result_bytes: 2048,
            max_output_tokens: 1000
        }
    );
    let higher = Limits {
        max_steps: Some(500),
        wall_clock_secs: Some(86_400),
        tool_result_bytes: Some(262_144),
        max_output_tokens: Some(16_777_216),
    };
    assert_eq!(limits(install, &higher), install);
}

#[test]
fn gw_u_050_the_runtimes_part_of_a_profile_is_its_snapshot() -> Result<()> {
    let mut stored = serde_json::to_value(profile()?)?;
    let skills = |filter: Value| -> Result<tqa_types::SkillsFilter> {
        let mut copy = serde_json::to_value(profile()?)?;
        copy["skills"] = filter;
        Ok(snapshot(&serde_json::from_value(copy)?)?.skills)
    };
    assert_eq!(skills(json!("all"))?, tqa_types::SkillsFilter::All);
    assert_eq!(skills(json!("none"))?, tqa_types::SkillsFilter::None);
    assert_eq!(
        skills(json!(["notes"]))?,
        tqa_types::SkillsFilter::Only(vec!["notes".into()])
    );
    stored["instructions"] = json!("Review only.");
    stored["tools"] = json!(["read_file"]);
    let sent = snapshot(&serde_json::from_value(stored.clone())?)?;
    assert_eq!(
        serde_json::to_value(&sent)?,
        json!({"name": "p", "instructions": "Review only.", "tools": ["read_file"], "skills": "all"})
    );
    // A stored copy the wire refuses (the validator never stores one) fails closed.
    stored["instructions"] = json!("");
    assert!(matches!(
        snapshot(&serde_json::from_value(stored)?),
        Err(GatewayError::Configuration(_))
    ));
    Ok(())
}

// TEST: GW-U-129
#[test]
fn gw_u_129_a_copy_stored_with_repositories_runs_with_its_own_tools_and_no_git_push() -> Result<()>
{
    let tools = |repositories: bool, listed: Option<Value>| -> Result<Option<Vec<String>>> {
        let mut copy = serde_json::to_value(profile()?)?;
        copy["repositories"] = json!(repositories);
        copy["tools"] = listed.unwrap_or(Value::Null);
        Ok(snapshot(&serde_json::from_value(copy)?)?.tools)
    };
    // A task's copy from an earlier release may still hold the removed switch: it grants nothing.
    for repositories in [false, true] {
        assert_eq!(
            tools(repositories, None)?,
            None,
            "no list: the runtime's own set"
        );
        assert_eq!(
            tools(repositories, Some(json!(["read_file"])))?,
            Some(vec!["read_file".to_owned()])
        );
    }
    assert!(
        !tqa_tools::builtin::BUILTIN_TOOL_NAMES.contains(&"git_push"),
        "git_push is gone"
    );
    Ok(())
}
