//! PROTO-U-016: one tool-name rule for tool catalogs and the action hash. A name is a builtin
//! (`[a-z][a-z0-9_]{0,63}`) or an MCP tool `mcp.<server>.<tool>`: the server under the MCP name rule, the tool the server's
//! own name (1 to 128 bytes of `[A-Za-z0-9_.-]`), split at the first two dots. Parser and schema agree on every case.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    MAX_MCP_TOOL_BYTES, PermissionContext, canonical_action_bytes, split_mcp_tool_name,
    valid_builtin_tool_name, valid_tool_name,
};

type TestResult = Result<(), Box<dyn Error>>;
/// Valid MCP names with their (server, tool) split, and invalid names.
type McpCases = (Vec<(String, (String, String))>, Vec<String>);

fn builtins() -> (Vec<String>, Vec<String>) {
    let valid = vec!["a".into(), "read_file".into(), "x9_".into(), "a".repeat(64)];
    let invalid = vec![
        String::new(),
        "A".into(),
        "1a".into(),
        "_a".into(),
        "a-b".into(),
        "a.b".into(),
        "a".repeat(65),
    ];
    (valid, invalid)
}
fn mcp() -> McpCases {
    let long_server = "abcdefghijklmnop";
    let long_tool = "t".repeat(MAX_MCP_TOOL_BYTES);
    let valid = vec![
        ("mcp.docs.search".into(), ("docs".into(), "search".into())),
        ("mcp.a.x".into(), ("a".into(), "x".into())),
        (
            "mcp.tracker.issues.create".into(),
            ("tracker".into(), "issues.create".into()),
        ),
        (
            "mcp.a_b.Get-Item_2.v1".into(),
            ("a_b".into(), "Get-Item_2.v1".into()),
        ),
        (
            format!("mcp.{long_server}.{long_tool}"),
            (long_server.into(), long_tool.clone()),
        ),
        ("mcp.docs..".into(), ("docs".into(), ".".into())),
    ];
    // `mcp` alone is a valid builtin name; only the `mcp.` prefix is the MCP namespace.
    let invalid = vec![
        "mcp.".into(),
        "mcp.docs".into(),
        "mcp.docs.".into(),
        "mcp..search".into(),
        "mcp.Docs.search".into(),
        "mcp.a__b.search".into(),
        "mcp.a_.search".into(),
        "mcp.1a.search".into(),
        format!("mcp.{long_server}q.search"),
        format!("mcp.docs.{long_tool}t"),
        "mcp.docs.a b".into(),
        "mcp.docs.é".into(),
        "mcp.docs.x/y".into(),
        "mcp.docs.x\u{0}".into(),
        "MCP.docs.search".into(),
        "mcpx.docs.search".into(),
    ];
    (valid, invalid)
}

fn context() -> Result<PermissionContext, Box<dyn Error>> {
    Ok(serde_json::from_value(
        json!({"conversation_id":"conversation",
        "run_id":"run","step":1,"attempt":1,"call_id":"call"}),
    )?)
}

#[test]
fn proto_u_016_the_rule_accepts_builtins_and_mcp_tools_only() {
    assert_eq!(MAX_MCP_TOOL_BYTES, 128);
    let (builtin_valid, builtin_invalid) = builtins();
    let (mcp_valid, mcp_invalid) = mcp();
    for name in &builtin_valid {
        assert!(valid_builtin_tool_name(name), "{name}");
        assert!(valid_tool_name(name), "{name}");
        assert_eq!(split_mcp_tool_name(name), None, "{name}");
    }
    for (name, (server, tool)) in &mcp_valid {
        assert!(valid_tool_name(name), "{name}");
        assert!(!valid_builtin_tool_name(name), "{name}");
        assert_eq!(
            split_mcp_tool_name(name),
            Some((server.as_str(), tool.as_str())),
            "{name}"
        );
    }
    for name in builtin_invalid.iter().chain(&mcp_invalid) {
        assert!(!valid_tool_name(name), "{name:?}");
        assert!(!valid_builtin_tool_name(name), "{name:?}");
        assert_eq!(split_mcp_tool_name(name), None, "{name:?}");
    }
}

#[test]
fn proto_u_016_the_action_hash_takes_every_valid_name_and_refuses_the_rest() -> TestResult {
    let context = context()?;
    let arguments = json!({"query":"x"});
    for (name, _) in mcp().0 {
        let bytes = canonical_action_bytes(&name, &arguments, &context)?;
        let envelope: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(
            envelope["tool"],
            json!(name),
            "the name is hashed unchanged"
        );
    }
    for name in builtins().0 {
        assert!(canonical_action_bytes(&name, &arguments, &context).is_ok());
    }
    for name in builtins().1.iter().chain(&mcp().1) {
        assert!(
            canonical_action_bytes(name, &arguments, &context).is_err(),
            "{name:?}"
        );
    }
    Ok(())
}
