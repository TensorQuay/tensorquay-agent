//! GW-U-118: where the install CA lives, and the places it may not.
use super::{HostKeysInput, ca_directory};
use crate::GatewayError;
use std::path::{Path, PathBuf};

fn place(configured: Option<&str>, skills: Option<&str>) -> Result<PathBuf, GatewayError> {
    ca_directory(
        HostKeysInput {
            ca_directory: configured.map(PathBuf::from),
        },
        Path::new("/srv/tqa"),
        skills.map(Path::new),
    )
}

// TEST: GW-U-118
#[test]
fn gw_u_118_the_ca_lives_under_storage_unless_configured_and_never_where_a_sandbox_reads()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(place(None, None)?, PathBuf::from("/srv/tqa/host-ca"));
    assert_eq!(
        place(Some("/var/lib/tqa-ca"), None)?,
        PathBuf::from("/var/lib/tqa-ca")
    );
    let message = |result: Result<PathBuf, GatewayError>| match result {
        Err(GatewayError::Configuration(message)) => message,
        _ => "accepted",
    };
    for relative in ["host-ca", "/srv/tqa/../tqa/host-ca"] {
        assert_eq!(
            message(place(Some(relative), None)),
            "host_keys.ca_directory must be an absolute path without ..",
            "{relative}"
        );
    }
    for mounted in [
        "/srv/tqa/workspaces/ca",
        "/srv/tqa/runtime-state",
        "/srv/tqa/skills/users/ca",
        "/opt/skills/ca",
    ] {
        assert_eq!(
            message(place(Some(mounted), Some("/opt/skills"))),
            "host_keys.ca_directory must not be inside a directory a sandbox mounts (workspaces, runtime state, skills)",
            "{mounted}"
        );
    }
    Ok(())
}
