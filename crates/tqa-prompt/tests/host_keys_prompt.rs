//! PROMPT-U-019 (ADR-0037 §3): a run with host keys is told each key's variable and address, and how to use it (the
//! Authorization header only; plain git for git hosts), never a placeholder. A run without keys gets the prompt it got
//! before (version 11 adds only these lines).
use tqa_prompt::{PROMPT_VERSION, WorkspaceFacts, system_prompt};
use tqa_types::{Environment, HostKeyGrant, NetworkEntry, NetworkSnapshot};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const GITHUB: &str = "tqa_ph_github0000000000000000000000000000000000000";
const API: &str = "tqa_ph_api0000000000000000000000000000000000000000";

fn render(host_keys: &[HostKeyGrant]) -> String {
    system_prompt(&WorkspaceFacts {
        root_display: "/workspace",
        os: "Linux",
        environment: &Environment::default(),
        network: &NetworkSnapshot {
            allowed: vec![NetworkEntry {
                host: "github.com".into(),
                ports: vec![443],
            }],
            more: 0,
        },
        commands: &[],
        host_keys,
    })
}

// TEST: PROMPT-U-019
#[test]
fn prompt_u_019_the_prompt_names_each_key_and_where_it_goes_never_a_placeholder() -> TestResult {
    let keys = [
        HostKeyGrant::new(
            "GITHUB_WORK".into(),
            ("github.com".into(), 443, Some("/acme".into())),
            ("basic".into(), Some("x-access-token".into())),
            GITHUB.into(),
        )?,
        HostKeyGrant::new(
            "API_TOKEN".into(),
            ("api.example.com".into(), 8443, None),
            ("bearer".into(), None),
            API.into(),
        )?,
    ];
    let without = render(&[]);
    let with = render(&keys);
    assert_eq!(PROMPT_VERSION, 14);
    // Without keys nothing changes; with them, the lines follow the network lines.
    assert!(with.starts_with(&without), "{with}");
    assert_eq!(
        &with[without.len()..],
        "Keys: these environment variables hold keys for web addresses. Use one only in the Authorization header \
         of a request to its address: `Authorization: Bearer $NAME` for Bearer; for Basic, the user shown and the \
         variable as the password. For a git address, use plain git (clone, fetch, push): git is already set up with \
         the key. The proxy puts the real key in, and it works nowhere else. Don't print the variables.\n\
         - GITHUB_WORK: https://github.com/acme (Basic, user x-access-token)\n\
         - API_TOKEN: https://api.example.com:8443 (Bearer)\n"
    );
    assert!(
        !with.contains("tqa_ph_"),
        "a placeholder never reaches the model"
    );
    Ok(())
}
