//! PROTO-U-012: MCP servers in `session.config` (revision 7, ADR-0011, ADR-0031). A server is a stdio process inside
//! the sandbox or a gateway connector; the connector's URL is never on the wire. Every bound is refused on parse and
//! by the exported schema alike.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    ClientFrame, DEFAULT_MCP_CALL_TIMEOUT_SECONDS, MAX_MCP_ARGS, MAX_MCP_CALL_TIMEOUT_SECONDS,
    MAX_MCP_ENV, MAX_MCP_ENV_VALUE_BYTES, MAX_MCP_NAME_BYTES, MAX_MCP_SERVERS,
    MAX_MCP_STRING_BYTES, MAX_MCP_TOOL_FILTER, MCP_NAME_RULE, protocol_schema, valid_mcp_name,
};

type TestResult = Result<(), Box<dyn Error>>;
/// One change to a server's wire form.
type Edit = Box<dyn Fn(&mut Value)>;

/// The two wire forms agreed in the revision 7 design, byte for byte.
fn stdio(name: &str) -> Value {
    json!({"name":name,"transport":{"kind":"stdio","command":"/usr/local/bin/docs-mcp","args":[],"env":{}},
        "call_timeout_seconds":60})
}
fn gateway(name: &str) -> Value {
    json!({"name":name,"transport":{"kind":"gateway","connector":name},"call_timeout_seconds":60})
}
fn config(mcp: Option<Value>) -> Value {
    let mut frame = json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":{"name":"local","protocol":"openai_chat","model":"m","relay_path":"/relay/r/local",
            "context_window":8192,"max_output":1024,"native_tools":true,"parallel_tools":true,"vision":false,
            "reasoning":null},
        "network":{"allowed":[],"more":0},
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    });
    if let Some(value) = mcp {
        frame["mcp"] = value;
    }
    frame
}
fn schema_accepts(value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}
fn parse(value: &Value) -> Result<ClientFrame, tqa_types::FrameError> {
    ClientFrame::parse(&serde_json::to_vec(value).unwrap_or_default())
}
/// One server changed by `edit`, inside an otherwise valid frame.
fn with(server: Value, edit: impl FnOnce(&mut Value)) -> Value {
    let mut server = server;
    edit(&mut server);
    config(Some(json!([server])))
}
/// Both the parser and the schema refuse the frame, and the parser names the MCP list.
fn assert_refused(frame: &Value) -> TestResult {
    let error = parse(frame)
        .err()
        .ok_or_else(|| format!("accepted: {frame}"))?;
    assert_eq!(error.code, tqa_types::ErrorCode::InvalidFrame, "{frame}");
    assert!(error.path.starts_with("$.mcp"), "{}: {frame}", error.path);
    assert!(!schema_accepts(frame)?, "the schema accepts {frame}");
    Ok(())
}
fn assert_accepted(frame: &Value) -> TestResult {
    let parsed = parse(frame).map_err(|e| format!("{e:?}: {frame}"))?;
    assert_eq!(&serde_json::to_value(&parsed)?, frame, "round trip");
    assert!(schema_accepts(frame)?, "the schema refuses {frame}");
    Ok(())
}

#[test]
fn proto_u_012_the_bounds_are_named_constants() {
    assert_eq!(MAX_MCP_SERVERS, 8);
    assert_eq!(MAX_MCP_NAME_BYTES, 16);
    assert_eq!(MAX_MCP_ARGS, 32);
    assert_eq!(MAX_MCP_ENV, 32);
    assert_eq!(MAX_MCP_TOOL_FILTER, 64);
    assert_eq!(MAX_MCP_STRING_BYTES, 1024);
    assert_eq!(MAX_MCP_ENV_VALUE_BYTES, 4096);
    assert_eq!(MAX_MCP_CALL_TIMEOUT_SECONDS, 600);
    assert_eq!(DEFAULT_MCP_CALL_TIMEOUT_SECONDS, 60);
    // One letter, then letters or digits each optionally after a single '_': no "__", no trailing '_'.
    assert_eq!(MCP_NAME_RULE, "^[a-z](_?[a-z0-9])*$");
}

#[test]
fn proto_u_012_absent_mcp_is_omitted_and_both_transports_round_trip() -> TestResult {
    assert_accepted(&config(None))?;
    assert_accepted(&config(Some(json!([stdio("docs"), gateway("jira")]))))?;
    let full = with(stdio("docs"), |s| {
        s["transport"]["args"] = json!(["--root", "/workspace"]);
        s["transport"]["env"] = json!({"LOG_LEVEL":"info","_X1":"y"});
        s["tools"] = json!({"allow":["search","fetch"]});
        s["call_timeout_seconds"] = json!(600);
    });
    assert_accepted(&full)?;
    assert_accepted(&with(gateway("jira"), |s| {
        s["tools"] = json!({"deny":["delete_issue"]});
    }))?;
    // In revision 7 the gateway sends connector == name, but a separate connector is a valid alias on the wire.
    assert_accepted(&with(gateway("tracker"), |s| {
        s["transport"]["connector"] = json!("jira");
    }))?;
    Ok(())
}

#[test]
fn proto_u_012_omitted_timeout_args_and_env_take_their_defaults() -> TestResult {
    let sparse = config(Some(json!([
        {"name":"docs","transport":{"kind":"stdio","command":"/usr/local/bin/docs-mcp"}},
        {"name":"jira","transport":{"kind":"gateway","connector":"jira"}}
    ])));
    let parsed = parse(&sparse).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        serde_json::to_value(&parsed)?["mcp"],
        json!([stdio("docs"), gateway("jira")]),
        "a 60 s timeout for both kinds; empty args and env"
    );
    assert!(schema_accepts(&sparse)?);
    Ok(())
}

#[test]
fn proto_u_012_the_name_rule_is_shared_by_names_and_connectors() -> TestResult {
    let sixteen = "abcdefghijklmnop";
    let seventeen = "abcdefghijklmnopq";
    for valid in ["a", "docs", "a1", "a_b", "a_b_c9", "x9_0", sixteen] {
        assert!(valid_mcp_name(valid), "{valid}");
        assert_accepted(&config(Some(json!([stdio(valid)]))))?;
        assert_accepted(&config(Some(json!([gateway(valid)]))))?;
    }
    for invalid in [
        "", "A", "Docs", "1a", "_a", "a__b", "a_", "a-b", "a.b", "a b", "é", seventeen,
    ] {
        assert!(!valid_mcp_name(invalid), "{invalid:?}");
        assert_refused(&config(Some(json!([stdio(invalid)]))))?;
        assert_refused(&config(Some(json!([gateway(invalid)]))))?;
        assert_refused(&with(gateway("jira"), |s| {
            s["transport"]["connector"] = json!(invalid);
        }))?;
    }
    Ok(())
}

#[test]
fn proto_u_012_at_most_eight_servers_with_unique_names_across_both_kinds() -> TestResult {
    let eight: Vec<_> = (0..8)
        .map(|i| {
            if i % 2 == 0 {
                stdio(&format!("s{i}"))
            } else {
                gateway(&format!("s{i}"))
            }
        })
        .collect();
    assert_accepted(&config(Some(json!(eight))))?;
    let mut nine = eight.clone();
    nine.push(gateway("s8"));
    assert_refused(&config(Some(json!(nine))))?;
    // A stdio server and a connector may not share a name: mcp__<name>__ must split back to one server.
    let duplicate = config(Some(json!([stdio("docs"), gateway("docs")])));
    let error = parse(&duplicate)
        .err()
        .ok_or("a duplicate name was accepted")?;
    assert!(error.path.starts_with("$.mcp"), "{}", error.path);
    Ok(())
}

#[test]
fn proto_u_012_stdio_command_args_and_env_are_bounded() -> TestResult {
    let at = |n: usize| format!("/{}", "c".repeat(n - 1));
    assert_accepted(&with(stdio("docs"), |s| {
        s["transport"]["command"] = json!(at(MAX_MCP_STRING_BYTES));
    }))?;
    for command in [
        json!(""),
        json!("docs-mcp"),
        json!("./docs-mcp"),
        json!(at(MAX_MCP_STRING_BYTES + 1)),
        json!(null),
    ] {
        assert_refused(&with(stdio("docs"), |s| {
            s["transport"]["command"] = command;
        }))?;
    }
    let arg = "a".repeat(MAX_MCP_STRING_BYTES);
    assert_accepted(&with(stdio("docs"), |s| {
        s["transport"]["args"] = json!(vec![arg.clone(); MAX_MCP_ARGS]);
    }))?;
    assert_refused(&with(stdio("docs"), |s| {
        s["transport"]["args"] = json!(vec!["a"; MAX_MCP_ARGS + 1]);
    }))?;
    assert_refused(&with(stdio("docs"), |s| {
        s["transport"]["args"] = json!([format!("{arg}a")]);
    }))?;

    let env = |pairs: Vec<(String, String)>| {
        Value::Object(
            pairs
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect(),
        )
    };
    let full: Vec<_> = (0..MAX_MCP_ENV)
        .map(|i| (format!("V{i}"), "v".repeat(MAX_MCP_ENV_VALUE_BYTES)))
        .collect();
    assert_accepted(&with(stdio("docs"), |s| s["transport"]["env"] = env(full)))?;
    let over: Vec<_> = (0..=MAX_MCP_ENV)
        .map(|i| (format!("V{i}"), String::new()))
        .collect();
    assert_refused(&with(stdio("docs"), |s| s["transport"]["env"] = env(over)))?;
    let long_name = format!("A{}", "B".repeat(63));
    assert_accepted(&with(stdio("docs"), |s| {
        s["transport"]["env"] = json!({ long_name.clone(): "" });
    }))?;
    for name in [
        "",
        "lower",
        "1A",
        "A-B",
        "TQA_TOKEN",
        "TQA_",
        &format!("{long_name}C"),
    ] {
        assert_refused(&with(stdio("docs"), |s| {
            s["transport"]["env"] = json!({ name: "x" });
        }))?;
    }
    assert_accepted(&with(stdio("docs"), |s| {
        s["transport"]["env"] = json!({"TQ_A":"x","XTQA_Y":"x"});
    }))?;
    assert_refused(&with(stdio("docs"), |s| {
        s["transport"]["env"] = json!({"V": "v".repeat(MAX_MCP_ENV_VALUE_BYTES + 1)});
    }))?;
    Ok(())
}

#[test]
fn proto_u_012_the_tool_filter_is_allow_or_deny_and_bounded() -> TestResult {
    let names = |n: usize| json!((0..n).map(|i| format!("t{i}")).collect::<Vec<_>>());
    for key in ["allow", "deny"] {
        // A name at the byte bound is accepted (mutants found this edge untested).
        assert_accepted(&with(gateway("jira"), |s| {
            s["tools"] = json!({ key: ["t".repeat(MAX_MCP_STRING_BYTES)] });
        }))?;
        assert_accepted(&with(gateway("jira"), |s| {
            s["tools"] = json!({ key: names(MAX_MCP_TOOL_FILTER) });
        }))?;
        for list in [
            names(MAX_MCP_TOOL_FILTER + 1),
            json!([""]),
            json!(["t".repeat(MAX_MCP_STRING_BYTES + 1)]),
            json!("search"),
        ] {
            assert_refused(&with(gateway("jira"), |s| {
                s["tools"] = json!({ key: list });
            }))?;
        }
    }
    for filter in [
        json!({"allow":["a"],"deny":["b"]}),
        json!({}),
        json!({"only":["a"]}),
        json!(["a"]),
    ] {
        assert_refused(&with(gateway("jira"), |s| s["tools"] = filter))?;
    }
    Ok(())
}

#[test]
fn proto_u_012_the_call_timeout_is_bounded_and_no_class_is_taken() -> TestResult {
    assert_accepted(&with(gateway("jira"), |s| {
        s["call_timeout_seconds"] = json!(1);
    }))?;
    for timeout in [json!(0), json!(601), json!(-1), json!(1.5), json!("60")] {
        assert_refused(&with(gateway("jira"), |s| {
            s["call_timeout_seconds"] = timeout;
        }))?;
    }
    // ADR-0035: the permission class only fed the removed policy.
    for class in [json!("execute"), json!("read")] {
        assert_refused(&with(gateway("jira"), |s| s["class"] = class))?;
    }
    Ok(())
}

#[test]
fn proto_u_012_unknown_fields_kinds_and_urls_are_refused() -> TestResult {
    let edits: Vec<Edit> = vec![
        Box::new(|s| s["url"] = json!("https://elsewhere.example/mcp")),
        Box::new(|s| s["transport"]["url"] = json!("https://elsewhere.example/mcp")),
        Box::new(|s| s["transport"]["kind"] = json!("http")),
        Box::new(|s| s["transport"]["kind"] = json!("Gateway")),
        Box::new(|s| s["transport"]["command"] = json!("/bin/sh")),
        Box::new(|s| {
            s["transport"] = json!({"connector":"jira"});
        }),
        Box::new(|s| {
            s["transport"] = json!({"kind":"gateway"});
        }),
        Box::new(|s| {
            if let Some(server) = s.as_object_mut() {
                server.remove("transport");
            }
        }),
        Box::new(|s| {
            if let Some(server) = s.as_object_mut() {
                server.remove("name");
            }
        }),
    ];
    for edit in edits {
        assert_refused(&with(gateway("jira"), |s| edit(s)))?;
    }
    assert_refused(&with(stdio("docs"), |s| {
        s["transport"]["connector"] = json!("docs");
    }))?;
    assert_refused(&config(Some(json!({"docs": stdio("docs")}))))?;
    Ok(())
}

#[test]
fn proto_u_012_servers_are_configured_only_by_session_config() -> TestResult {
    let update =
        json!({"type":"session.update","route":config(None)["route"],"mcp":[gateway("jira")]});
    assert!(
        parse(&update).is_err(),
        "a change applies only with a new session.config"
    );
    assert!(!schema_accepts(&update)?);
    Ok(())
}

#[test]
fn proto_u_012_wrong_json_types_are_refused_with_the_list_named() -> TestResult {
    assert_refused(&config(Some(json!("docs"))))?;
    assert_refused(&config(Some(json!([stdio("docs"), 7]))))?;
    assert_refused(&with(stdio("docs"), |s| s["name"] = json!(7)))?;
    assert_refused(&with(stdio("docs"), |s| {
        s["transport"]["args"] = json!("--root");
    }))?;
    assert_refused(&with(stdio("docs"), |s| {
        s["transport"]["env"] = json!(["A=1"]);
    }))?;
    assert_refused(&with(gateway("jira"), |s| {
        s["tools"] = json!({"allow": [1]});
    }))?;
    Ok(())
}
