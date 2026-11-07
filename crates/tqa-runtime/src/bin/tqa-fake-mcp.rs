//! A scriptable stdio MCP server for tests (TOOL-I-012, RT-I-008); built only with the `testing` feature.
//!
//! - `FAKE_MCP_TOOLS`: a JSON array of tools for `tools/list`; the default lists `echo`, `slow`, `crash`, `fail`,
//!   `structured` and `image`. `FAKE_MCP_TOOLS_FILE` names a file holding the array instead, for lists too large for
//!   an environment value.
//! - `FAKE_MCP_MODE`: `normal` (default), `hang_handshake`, `oversized_line`, `garbage`, `exit_on_start`,
//!   `list_changed` (announces a changed tool list after the handshake), `sampling` (asks the client for sampling after
//!   the handshake and carries on), `ignore_eof` (keeps running after its input closes) or `unknown_ok` (answers a call
//!   to an unknown tool as a success).
//! - `FAKE_MCP_LOG`: a file that receives each incoming method name, one per line.
//!
//! Tools: `echo` returns its `text`; `slow` sleeps `ms` then returns `done`; `crash` exits with status 3; `fail` returns
//! `isError`; `structured` returns `structuredContent`; `image` returns an image part; `env` returns its environment's names.

use std::{
    io::{BufRead, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};

fn main() {
    let mode = std::env::var("FAKE_MCP_MODE").unwrap_or_else(|_| "normal".into());
    if mode == "exit_on_start" {
        std::process::exit(2);
    }
    let out = Arc::new(Mutex::new(std::io::stdout()));
    if mode == "garbage" {
        write_line(&out, "this is not json");
    }
    let tools: Value = std::env::var("FAKE_MCP_TOOLS")
        .ok()
        .or_else(|| {
            std::env::var("FAKE_MCP_TOOLS_FILE")
                .ok()
                .and_then(|path| std::fs::read_to_string(path).ok())
        })
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(default_tools);
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = message["method"].as_str().unwrap_or("").to_owned();
        log(&method);
        let id = message["id"].clone();
        match method.as_str() {
            "initialize" => match mode.as_str() {
                "hang_handshake" => {}
                "oversized_line" => write_line(&out, &"x".repeat(2 * 1024 * 1024)),
                _ => respond(
                    &out,
                    &id,
                    json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true}},
                        "serverInfo":{"name":"fake","version":"1"}}),
                ),
            },
            "notifications/initialized" if mode == "sampling" => {
                write_line(
                    &out,
                    &json!({"jsonrpc":"2.0","id":"fake-sampling","method":"sampling/createMessage",
                        "params":{"messages":[],"maxTokens":1}})
                    .to_string(),
                );
            }
            "notifications/initialized" if mode == "list_changed" => {
                write_line(
                    &out,
                    &json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})
                        .to_string(),
                );
            }
            "tools/list" => respond(&out, &id, json!({"tools": tools})),
            "tools/call" => call(&out, id, &message["params"], mode == "unknown_ok"),
            "ping" => respond(&out, &id, json!({})),
            _ => {}
        }
    }
    if mode == "ignore_eof" {
        loop {
            std::thread::park();
        }
    }
}

fn call(out: &Arc<Mutex<std::io::Stdout>>, id: Value, params: &Value, unknown_ok: bool) {
    let arguments = &params["arguments"];
    match params["name"].as_str().unwrap_or("") {
        "echo" => respond(out, &id, text(arguments["text"].as_str().unwrap_or(""))),
        "slow" => {
            let (out, ms) = (out.clone(), arguments["ms"].as_u64().unwrap_or(1000));
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                respond(&out, &id, text("done"));
            });
        }
        "crash" => std::process::exit(3),
        "env" => {
            let mut names: Vec<String> = std::env::vars().map(|(name, _)| name).collect();
            names.sort();
            respond(out, &id, text(&names.join("\n")));
        }
        "fail" => respond(
            out,
            &id,
            json!({"content":[{"type":"text","text":"boom"}],"isError":true}),
        ),
        "structured" => respond(
            out,
            &id,
            json!({"content":[],"structuredContent":{"a":1,"b":[true]}}),
        ),
        "image" => respond(
            out,
            &id,
            json!({"content":[{"type":"image","data":"iVBORw0KGgo=","mimeType":"image/png"}]}),
        ),
        _ if unknown_ok => respond(out, &id, text("ok")),
        _ => respond(
            out,
            &id,
            json!({"content":[{"type":"text","text":"unknown tool"}],"isError":true}),
        ),
    }
}

fn text(value: &str) -> Value {
    json!({"content":[{"type":"text","text":value}]})
}

fn respond(out: &Arc<Mutex<std::io::Stdout>>, id: &Value, result: Value) {
    let mut frame = json!({"jsonrpc":"2.0","id":id});
    frame["result"] = result;
    write_line(out, &frame.to_string());
}

fn write_line(out: &Arc<Mutex<std::io::Stdout>>, line: &str) {
    if let Ok(mut out) = out.lock() {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

fn log(method: &str) {
    if let Ok(path) = std::env::var("FAKE_MCP_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(file, "{method}");
    }
}

fn default_tools() -> Value {
    let object = json!({"type":"object","properties":{}});
    json!([
        {"name":"echo","description":"Echo the text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}},
        {"name":"slow","description":"Sleep","inputSchema":{"type":"object","properties":{"ms":{"type":"integer"}}}},
        {"name":"crash","description":"Exit","inputSchema":object},
        {"name":"fail","description":"Fail","inputSchema":object},
        {"name":"structured","description":"Structured","inputSchema":object},
        {"name":"image","description":"Image","inputSchema":object,"annotations":{"openWorldHint":true}}
    ])
}
