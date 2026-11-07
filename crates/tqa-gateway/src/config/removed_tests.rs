//! GW-U-082 (ADR-0035): per-call permissions are gone, so the settings that configured them stop the gateway at start
//! with a message that names what to remove and why, instead of being silently ignored: `[permissions]`, a profile's
//! `policy` and `rules`, a connector's or MCP server's `class`, and `[sandbox] park_after_seconds`.
use crate::{GatewayConfig, GatewayError};

fn load(extra: &str) -> Result<GatewayConfig, GatewayError> {
    let image = format!("sha256:{}", "a".repeat(64));
    GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{extra}"
    ))
}

// TEST: GW-U-082
#[test]
fn gw_u_082_removed_permission_settings_stop_start_up_naming_adr_0035()
-> Result<(), Box<dyn std::error::Error>> {
    for (extra, named) in [
        ("[permissions]\npolicy='workspace-write'\n", "[permissions]"),
        // Right after the fixture's [sandbox] table, so the key is in it.
        ("park_after_seconds=60\n", "park_after_seconds"),
        (
            "[permissions]\napproval_timeout_seconds=60\n",
            "[permissions]",
        ),
        (
            "[[profiles]]\nname='team'\ndisplay_name='Team'\npolicy='read-only'\n",
            "policy",
        ),
        (
            "[[profiles]]\nname='team'\ndisplay_name='Team'\nrules=[{tool='shell', decision='deny'}]\n",
            "rules",
        ),
        (
            "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nclass='read'\n",
            "class",
        ),
        (
            "[[connectors]]\nname='tracker'\nurl='https://tracker.example/mcp'\nauth={scheme='none'}\nclass='read'\n",
            "class",
        ),
    ] {
        match load(extra) {
            Err(GatewayError::Configuration(message)) => {
                assert!(
                    message.starts_with("gateway.toml: "),
                    "names the file: {message}"
                );
                assert!(message.contains("ADR-0035"), "{extra}: {message}");
                assert!(message.contains(named), "{extra}: {message}");
            }
            other => {
                return Err(format!("{extra}: expected a start-up refusal, got {other:?}").into());
            }
        }
    }
    // Without them the file loads.
    assert!(load("[[profiles]]\nname='team'\ndisplay_name='Team'\n").is_ok());
    Ok(())
}

// TEST: GW-U-083
#[test]
fn gw_u_083_a_profile_listing_git_push_or_setting_repositories_says_to_use_plain_git()
-> Result<(), Box<dyn std::error::Error>> {
    for (profile, named) in [
        ("tools=['read_file','git_push']\n", "git_push"),
        ("tools=['read_file']\nrepositories=true\n", "repositories"),
        ("repositories=false\n", "repositories"),
    ] {
        match load(&format!(
            "[[profiles]]\nname='team'\ndisplay_name='Team'\n{profile}"
        )) {
            Err(GatewayError::Configuration(message)) => {
                assert!(message.starts_with("gateway.toml: "), "{message}");
                assert!(
                    message.contains(named) && message.contains("plain git and a key under Keys"),
                    "{message}"
                );
            }
            other => return Err(format!("expected a start-up refusal, got {other:?}").into()),
        }
    }
    Ok(())
}
