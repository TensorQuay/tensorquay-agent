//! TOOL-I-013 (runtime side), the bounds: a connector's tool listing is bounded exactly, on one page and across pages,
//! and a null cursor ends it; a JSON answer and an event stream are bounded as they arrive, across chunks, with an
//! event's data lines joined by a newline. The proxy is the scripted HTTP fake of `support/mcp_proxy.rs`.
#[path = "support/mcp_proxy.rs"]
mod mcp_proxy;
use mcp_proxy::*;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use tqa_runtime::testing::GatewayServer;

const BOUND: usize = 1024 * 1024;

/// A tool the proxy lists.
fn tool(index: usize) -> Value {
    json!({"name":format!("tool_{index}"),"description":"d","inputSchema":{"type":"object"}})
}
/// The handshake, then `tools/list` pages by cursor: the first request gets page 0, cursor `p<n>` gets page `n`. Each
/// page's `nextCursor` is written as given (`None` leaves it out).
fn listing(pages: Vec<(usize, Option<Value>)>) -> Script {
    Arc::new(move |seen: &Seen, index| {
        let id = seen.body["id"].clone();
        match seen.body["method"].as_str() {
            Some("tools/list") => {
                let page = seen.body["params"]["cursor"]
                    .as_str()
                    .and_then(|cursor| cursor.strip_prefix('p'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0);
                let (count, next) = pages.get(page).cloned().unwrap_or((0, None));
                let start: usize = pages[..page.min(pages.len())].iter().map(|p| p.0).sum();
                let mut result =
                    json!({"tools":(start..start + count).map(tool).collect::<Vec<_>>()});
                if let Some(next) = next {
                    result["nextCursor"] = next;
                }
                json_answer(200, &json!({"jsonrpc":"2.0","id":id,"result":result}))
            }
            _ => handshake_then(|_, _| json_answer(500, &json!({})))(seen, index),
        }
    })
}
async fn connect(origin: &url::Url) -> Result<GatewayServer, tqa_runtime::testing::McpFailure> {
    GatewayServer::connect(
        origin,
        "tracker",
        token(),
        HANDSHAKE,
        Duration::from_secs(5),
    )
    .await
}
fn list_requests(seen: &Arc<Mutex<Vec<Seen>>>) -> usize {
    seen.lock().map_or(0, |seen| {
        seen.iter()
            .filter(|s| s.body["method"] == "tools/list")
            .count()
    })
}

#[tokio::test]
async fn tool_i_013_the_tool_bound_is_exact_on_one_page_and_across_pages() -> TestResult {
    // Exactly 64 on one page, and 40 + 24 across two, are accepted.
    for pages in [vec![(64, None)], vec![(40, Some(json!("p1"))), (24, None)]] {
        let (origin, _) = proxy(listing(pages)).await?;
        assert_eq!(connect(&origin).await?.tools().len(), 64);
    }
    // One past the bound, on one page or only when the pages add up, is refused.
    for pages in [vec![(65, None)], vec![(40, Some(json!("p1"))), (25, None)]] {
        let (origin, _) = proxy(listing(pages)).await?;
        let refused = connect(&origin).await.err().ok_or("65 tools accepted")?;
        assert!(
            matches!(refused, tqa_runtime::testing::McpFailure::OverLimit),
            "{refused:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn tool_i_013_a_null_next_cursor_ends_the_listing() -> TestResult {
    let (origin, seen) = proxy(listing(vec![(3, Some(Value::Null))])).await?;
    assert_eq!(connect(&origin).await?.tools().len(), 3);
    assert_eq!(list_requests(&seen), 1, "a null cursor is the last page");
    let (origin, seen) = proxy(listing(vec![(3, Some(json!("p1"))), (2, None)])).await?;
    assert_eq!(connect(&origin).await?.tools().len(), 5);
    assert_eq!(
        list_requests(&seen),
        2,
        "a string cursor asks for the next page"
    );
    Ok(())
}

/// A JSON answer to `id` whose whole body is exactly `size` bytes.
fn answer_of_size(id: &Value, size: usize) -> Answer {
    let head =
        format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":""#);
    let tail = r#""}]}}"#;
    let text = "x".repeat(size.saturating_sub(head.len() + tail.len()));
    Answer {
        status: 200,
        headers: vec![("content-type".into(), "application/json".into())],
        body: format!("{head}{text}{tail}").into_bytes(),
        delay: Duration::ZERO,
        split: Some((4096, Duration::from_millis(20))),
    }
}

#[tokio::test]
async fn tool_i_013_a_json_answer_of_exactly_the_bound_is_read_across_chunks() -> TestResult {
    let (origin, _) = proxy(handshake_then(|seen: &Seen, _| {
        answer_of_size(&seen.body["id"], BOUND)
    }))
    .await?;
    let output = call(&connect(&origin).await?).await?;
    assert!(!output.is_error && output.text.len() > BOUND / 2);
    let (origin, _) = proxy(handshake_then(|seen: &Seen, _| {
        answer_of_size(&seen.body["id"], BOUND + 1)
    }))
    .await?;
    let error = call(&connect(&origin).await?)
        .await
        .err()
        .ok_or("one byte past the bound accepted")?;
    assert!(
        error.model_message().starts_with("mcp_error: "),
        "{}",
        error.model_message()
    );
    Ok(())
}

fn events(body: String, split: Option<(usize, Duration)>) -> Answer {
    Answer {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        body: body.into_bytes(),
        delay: Duration::ZERO,
        split,
    }
}
fn answer_event(id: &Value, text: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":text}]}})
    )
}
/// One answer event split over two `data:` lines between JSON tokens, whose joined data is exactly `size` bytes.
fn two_line_answer(id: &Value, size: usize) -> String {
    let first = format!(
        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":"found"}}],"#
    );
    let open = r#""pad":""#;
    let close = r#""}}"#;
    let half = size / 2;
    let first = format!("{first}{}", " ".repeat(half.saturating_sub(first.len())));
    let pad = "y".repeat(size.saturating_sub(first.len() + 1 + open.len() + close.len()));
    format!("data: {first}\ndata: {open}{pad}{close}\n\n")
}

#[tokio::test]
async fn tool_i_013_an_event_stream_bounds_the_joined_data_of_one_event() -> TestResult {
    let (origin, _) = proxy(handshake_then(|seen: &Seen, _| {
        events(two_line_answer(&seen.body["id"], BOUND), None)
    }))
    .await?;
    let output = call(&connect(&origin).await?).await?;
    assert_eq!(output.text, "found", "data of exactly the bound is read");
    let (origin, _) = proxy(handshake_then(|seen: &Seen, _| {
        events(two_line_answer(&seen.body["id"], BOUND + 1), None)
    }))
    .await?;
    let error = call(&connect(&origin).await?)
        .await
        .err()
        .ok_or("joined data past the bound accepted")?;
    assert!(
        error.model_message().starts_with("mcp_error: "),
        "{}",
        error.model_message()
    );
    Ok(())
}

#[tokio::test]
async fn tool_i_013_an_event_stream_bounds_a_pending_line_and_reads_across_chunks() -> TestResult {
    // A large notification first, so the answer arrives in a later chunk after an unfinished line.
    let (origin, _) = proxy(handshake_then(|seen: &Seen, _| {
        let notice = json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"p":"n".repeat(300_000)}});
        events(
            format!("data: {notice}\n\n{}", answer_event(&seen.body["id"], "later")),
            Some((1000, Duration::from_millis(50))),
        )
    }))
    .await?;
    assert_eq!(call(&connect(&origin).await?).await?.text, "later");
    // A comment line of exactly the bound, its newline written later: pending reaches the bound and is still read.
    let (origin, _) = proxy(handshake_then(|seen: &Seen, _| {
        let comment = format!(":{}", "c".repeat(BOUND - 1));
        events(
            format!("{comment}\n{}", answer_event(&seen.body["id"], "after")),
            Some((BOUND, Duration::from_millis(100))),
        )
    }))
    .await?;
    assert_eq!(call(&connect(&origin).await?).await?.text, "after");
    // A comment line past the bound is refused, though no data grows, whether its newline comes with the chunk that
    // crosses the bound or never comes (then the pending check refuses it, not the end of the stream).
    for tail in ["\n", ""] {
        let (origin, _) = proxy(handshake_then(move |seen: &Seen, _| {
            oversized_comment(&seen.body["id"], tail)
        }))
        .await?;
        let error = call(&connect(&origin).await?)
            .await
            .err()
            .ok_or("an oversized line accepted")?;
        assert!(
            error.model_message().contains("1 MiB message bound"),
            "{}",
            error.model_message()
        );
    }
    Ok(())
}

/// A comment line past the bound, then `tail`; after a newline, an answer follows.
fn oversized_comment(id: &Value, tail: &str) -> Answer {
    let comment = format!(":{}", "c".repeat(BOUND + 10));
    let rest = if tail.is_empty() {
        String::new()
    } else {
        answer_event(id, "never")
    };
    events(format!("{comment}{tail}{rest}"), None)
}

#[tokio::test]
async fn tool_i_013_data_lines_join_with_a_newline() -> TestResult {
    // A string split over two data lines becomes a raw newline inside it: invalid JSON, so that event is dropped and
    // the next one answers. Gluing the lines would read "first" instead.
    let (origin, _) = proxy(handshake_then(|seen: &Seen, _| {
        let id = &seen.body["id"];
        events(
            format!(
                "data: {{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"fir\ndata: st\"}}]}}}}\n\n{}",
                answer_event(id, "second")
            ),
            None,
        )
    }))
    .await?;
    assert_eq!(call(&connect(&origin).await?).await?.text, "second");
    Ok(())
}
