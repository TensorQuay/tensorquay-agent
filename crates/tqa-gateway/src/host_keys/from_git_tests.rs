//! GW-U-112 (ADR-0037 §7): the name a Git credential's host key gets from its remote.
use super::{base_name, numbered};
use crate::host_keys::rules::{address, name};

fn named(remote: &str) -> String {
    base_name(&address(remote).unwrap_or_else(|_| unreachable!("a valid remote in the test")))
}

// TEST: GW-U-112
#[test]
fn gw_u_112_a_git_credentials_key_is_named_from_its_remote() {
    for (remote, expected) in [
        ("https://github.com/acme/tools.git", "GITHUB_ACME_TOOLS"),
        (
            "https://gitlab.example.com/group/sub-group/repo",
            "GITLAB_GROUP_SUB_GROUP_REPO",
        ),
        ("https://github.com", "GITHUB"),
        // A GIT_ start is reserved: the name gets a REPO_ prefix.
        ("https://git.example.org/team/app.git", "REPO_GIT_TEAM_APP"),
        // A name may not start with a digit.
        ("https://123.example.org/x", "REPO_123_X"),
        ("https://gitea/acme/über.git", "GITEA_ACME_BER"),
    ] {
        let got = named(remote);
        assert_eq!(got, expected, "{remote}");
        assert!(name(&got).is_ok(), "{got}");
    }
    let long = named(&format!(
        "https://github.com/{}/{}",
        "a".repeat(60),
        "b".repeat(60)
    ));
    assert!(long.len() <= 64 && name(&long).is_ok(), "{long}");
    // Taken: numbered, still within 64 and still a name.
    assert_eq!(numbered("GITHUB_ACME_TOOLS", 1), "GITHUB_ACME_TOOLS");
    assert_eq!(numbered("GITHUB_ACME_TOOLS", 2), "GITHUB_ACME_TOOLS_2");
    let numbered_long = numbered(&long, 37);
    assert!(
        numbered_long.len() <= 64 && numbered_long.ends_with("_37") && name(&numbered_long).is_ok()
    );
}
