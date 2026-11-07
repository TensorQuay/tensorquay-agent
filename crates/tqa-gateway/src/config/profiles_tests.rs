//! GW-U-049 (the file path): an invalid `[[profiles]]` entry stops start-up with the API's reason code.
use std::error::Error;

use crate::GatewayConfig;

fn load(entries: &str) -> Result<GatewayConfig, crate::GatewayError> {
    let image = format!("sha256:{}", "a".repeat(64));
    GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{entries}"
    ))
}

fn entry(name: &str, extra: &str) -> String {
    format!("[[profiles]]\nname='{name}'\ndisplay_name='Team'\n{extra}\n")
}

#[test]
fn gw_u_049_an_invalid_profile_entry_stops_start_up_with_its_reason() -> Result<(), Box<dyn Error>>
{
    let loaded = load(&[entry("team", "tools=['read_file']"), entry("docs", "")].concat())?;
    let names: Vec<&str> = loaded
        .profiles()
        .iter()
        .map(|profile| profile.name.as_str())
        .collect();
    assert_eq!(names, ["team", "docs"]);
    for (entries, code) in [
        (entry("Team", ""), "(name_invalid)"),
        (entry("team", "tools=['teleport']"), "(unknown_reference)"),
        (
            entry("team", "limits={max_steps=0}"),
            "(limit_out_of_range)",
        ),
        (entry("team", "instructions=''"), "(value_empty)"),
        (
            [entry("team", ""), entry("team", "")].concat(),
            "(name_taken)",
        ),
        (
            (0..65)
                .map(|index| entry(&format!("p{index}"), ""))
                .collect::<String>(),
            "(profile_count)",
        ),
    ] {
        let message = load(&entries).err().ok_or("accepted")?.to_string();
        assert!(message.contains(code), "{code}: {message}");
    }
    Ok(())
}

#[test]
fn gw_u_049_each_reason_the_validator_gives_is_named_at_start_up() {
    let named = [
        crate::InvalidReason::NameInvalid,
        crate::InvalidReason::DisplayNameInvalid,
        crate::InvalidReason::ValueTooLong,
        crate::InvalidReason::ValueNul,
        crate::InvalidReason::ValueEmpty,
        crate::InvalidReason::RouteNameInvalid,
        crate::InvalidReason::ListInvalid,
        crate::InvalidReason::UnknownReference,
        crate::InvalidReason::LimitOutOfRange,
    ];
    for reason in crate::InvalidReason::ALL {
        let expected = if named.contains(&reason) {
            format!("a [[profiles]] entry is invalid ({})", reason.as_str())
        } else {
            "a [[profiles]] entry is invalid".to_owned()
        };
        assert_eq!(super::refusal(reason), expected, "{reason:?}");
    }
}
