//! GW-U-038: the MCP proxy's message rules (ADR-0031 decision 2).
use super::{
    Unusable, Verdict, check, filter_answer, forward_answer_header, forward_request_header, parse,
};
use crate::connectors::ToolFilter;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn body(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

fn allow(tools: &[&str]) -> ToolFilter {
    ToolFilter {
        allow: Some(tools.iter().map(|tool| (*tool).to_owned()).collect()),
        deny: None,
    }
}

fn deny(tools: &[&str]) -> ToolFilter {
    ToolFilter {
        allow: None,
        deny: Some(tools.iter().map(|tool| (*tool).to_owned()).collect()),
    }
}

#[test]
fn gw_u_038_one_message_or_a_bounded_batch_of_json_rpc_2_0() -> TestResult {
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    assert_eq!(parse(&body(&call))?.label(), "tools/list");
    let notification = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    assert_eq!(
        parse(&body(&notification))?.label(),
        "notifications/initialized"
    );
    let batch: Vec<Value> = (0..32)
        .map(|id| json!({"jsonrpc": "2.0", "id": id, "method": "ping"}))
        .collect();
    assert_eq!(parse(&body(&json!(batch)))?.label(), "batch");
    for invalid in [
        json!([]),
        json!(
            (0..33)
                .map(|id| json!({"jsonrpc": "2.0", "id": id, "method": "ping"}))
                .collect::<Vec<_>>()
        ),
        json!({"id": 1, "method": "ping"}),
        json!({"jsonrpc": "1.0", "id": 1, "method": "ping"}),
        json!({"jsonrpc": "2.0", "id": 1.5, "method": "ping"}),
        json!({"jsonrpc": "2.0", "id": {"x": 1}, "method": "ping"}),
        json!({"jsonrpc": "2.0", "id": 1, "method": 7}),
        json!([1]),
        json!("ping"),
    ] {
        assert_eq!(
            parse(&body(&invalid)).err(),
            Some(Unusable::Invalid),
            "{invalid}"
        );
    }
    assert_eq!(parse(b"not json").err(), Some(Unusable::Invalid));
    let big = format!(
        "{{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"pad\":\"{}\"}}",
        "x".repeat(1 << 20)
    );
    assert_eq!(parse(big.as_bytes()).err(), Some(Unusable::TooLarge));
    assert_eq!(
        parse(&body(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "resources/list"})
        ))?
        .label(),
        "other"
    );
    Ok(())
}

#[test]
fn gw_u_038_only_the_tool_methods_pass_and_a_refusal_answers_every_request() -> TestResult {
    for method in [
        "initialize",
        "notifications/initialized",
        "ping",
        "tools/list",
        "notifications/cancelled",
    ] {
        let message = parse(&body(&json!({"jsonrpc": "2.0", "id": 1, "method": method})))?;
        assert_eq!(check(&message, None), Verdict::Forward, "{method}");
    }
    for method in [
        "resources/list",
        "prompts/get",
        "completion/complete",
        "logging/setLevel",
        "sampling/createMessage",
    ] {
        let message = parse(&body(
            &json!({"jsonrpc": "2.0", "id": "a", "method": method}),
        ))?;
        assert_eq!(
            check(&message, None),
            Verdict::Refuse(Some(
                json!({"jsonrpc": "2.0", "id": "a", "error": {"code": -32601, "message": "method not available through this connector"}})
            )),
            "{method}"
        );
    }
    // An answer to a server request (sampling, elicitation) never passes.
    let answer = parse(&body(&json!({"jsonrpc": "2.0", "id": 9, "result": {}})))?;
    assert!(matches!(check(&answer, None), Verdict::Refuse(Some(_))));
    // A refused notification has nobody to answer.
    let note = parse(&body(
        &json!({"jsonrpc": "2.0", "method": "notifications/message"}),
    ))?;
    assert_eq!(check(&note, None), Verdict::Refuse(None));
    // One refused message refuses the batch; every request in it is answered.
    let batch = parse(&body(&json!([
        {"jsonrpc": "2.0", "id": 1, "method": "ping"},
        {"jsonrpc": "2.0", "id": 2, "method": "resources/read"},
        {"jsonrpc": "2.0", "method": "notifications/initialized"}
    ])))?;
    let Verdict::Refuse(Some(Value::Array(answers))) = check(&batch, None) else {
        return Err("the batch is refused".into());
    };
    assert_eq!(answers.len(), 2);
    assert_eq!(
        answers[0]["error"]["message"],
        "refused with the rest of the batch"
    );
    assert_eq!(answers[1]["error"]["code"], -32601);
    Ok(())
}

#[test]
fn gw_u_038_a_call_must_name_a_tool_the_connector_offers() -> TestResult {
    let call = |name: Value| {
        body(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": name}}))
    };
    let search = parse(&call(json!("search")))?;
    let delete = parse(&call(json!("delete_issue")))?;
    assert_eq!(check(&search, None), Verdict::Forward);
    assert_eq!(check(&search, Some(&allow(&["search"]))), Verdict::Forward);
    assert_eq!(
        check(&search, Some(&deny(&["delete_issue"]))),
        Verdict::Forward
    );
    for (message, filter) in [
        (&delete, allow(&["search"])),
        (&delete, deny(&["delete_issue"])),
    ] {
        assert_eq!(
            check(message, Some(&filter)),
            Verdict::Refuse(Some(
                json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32602, "message": "tool not available through this connector"}})
            ))
        );
    }
    // A call without a tool name, or an empty filter, offers nothing.
    let nameless = parse(&body(
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {}}),
    ))?;
    assert!(matches!(check(&nameless, None), Verdict::Refuse(Some(_))));
    let empty = ToolFilter {
        allow: None,
        deny: None,
    };
    assert!(matches!(
        check(&search, Some(&empty)),
        Verdict::Refuse(Some(_))
    ));
    Ok(())
}

#[test]
fn gw_u_038_tools_list_answers_are_filtered_and_nothing_else_is() -> TestResult {
    let request = parse(&body(&json!([
        {"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 2, "method": "ping"}
    ])))?;
    let ids = request.tools_list_ids();
    assert_eq!(ids, [json!(1)]);
    let tools = json!({"tools": [{"name": "search"}, {"name": "delete_issue"}, {"title": "nameless"}], "nextCursor": "c"});
    let mut answer = json!([{"jsonrpc": "2.0", "id": 1, "result": tools}, {"jsonrpc": "2.0", "id": 2, "result": tools}]);
    filter_answer(&mut answer, &ids, Some(&deny(&["delete_issue"])));
    assert_eq!(answer[0]["result"]["tools"], json!([{"name": "search"}]));
    assert_eq!(answer[0]["result"]["nextCursor"], "c");
    assert_eq!(answer[1]["result"], tools, "a ping answer is untouched");
    let mut single = json!({"jsonrpc": "2.0", "id": 1, "result": tools});
    filter_answer(&mut single, &ids, Some(&allow(&["delete_issue"])));
    assert_eq!(single["result"]["tools"], json!([{"name": "delete_issue"}]));
    let mut untouched = json!({"jsonrpc": "2.0", "id": 1, "result": tools});
    filter_answer(&mut untouched, &ids, None);
    assert_eq!(untouched["result"], tools, "no filter, no change");
    let mut error = json!({"jsonrpc": "2.0", "id": 1, "error": {"code": 1, "message": "x"}});
    filter_answer(&mut error, &ids, Some(&allow(&["search"])));
    assert_eq!(error["error"]["code"], 1);
    Ok(())
}

#[test]
fn gw_u_038_only_protocol_headers_cross_the_proxy() {
    for name in [
        "Content-Type",
        "accept",
        "MCP-Protocol-Version",
        "Mcp-Session-Id",
    ] {
        assert!(forward_request_header(name), "{name}");
    }
    for name in [
        "Authorization",
        "Proxy-Authorization",
        "Cookie",
        "X-Api-Key",
        "Connection",
        "Transfer-Encoding",
        "Host",
        "X-Forwarded-For",
    ] {
        assert!(!forward_request_header(name), "{name}");
    }
    for name in ["content-type", "Mcp-Session-Id"] {
        assert!(forward_answer_header(name), "{name}");
    }
    for name in ["Set-Cookie", "WWW-Authenticate", "Server", "Location"] {
        assert!(!forward_answer_header(name), "{name}");
    }
}
