//! LLM-U-052 (ADR-0039): an image on a tool's result message, as each protocol takes it. Anthropic Messages puts it
//! inside the `tool_result`; Chat Completions and Responses send the result's text, then, after the run of tool results,
//! one user message holding only a fixed label and the image, never the person's words.
use serde_json::{Value, json};
use std::error::Error;
use tqa_llm::{CompletionRequest, ContentPart, Message, Registry, Role, ToolCall, encode_request};

type TestResult = Result<(), Box<dyn Error>>;

const PNG: [u8; 4] = [0x89, b'P', b'N', b'G'];
const PNG_URL: &str = "data:image/png;base64,iVBORw==";

fn route(protocol: &str, vision: bool) -> Result<tqa_llm::Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='{protocol}'\nbase_url='http://127.0.0.1:9/v1'\nmodel='m'\n\
         auth={{kind='none'}}\nmax_output=100\nvision={vision}\n"
    ))?;
    Ok(registry.get("test").ok_or("missing route")?.clone())
}

fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

/// A user turn, an assistant asking for a screenshot and a file in one response, both results (the screenshot's with
/// its image), and the assistant's next words.
fn request() -> CompletionRequest {
    let mut asked = message(Role::Assistant, "");
    asked.tool_calls = vec![
        ToolCall {
            id: "c1".into(),
            name: "computer".into(),
            arguments: r#"{"action":"screenshot"}"#.into(),
        },
        ToolCall {
            id: "c2".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a"}"#.into(),
        },
    ];
    let mut shot = message(Role::Tool, "Screenshot 2×2.");
    shot.tool_call_id = Some("c1".into());
    shot.content.push(ContentPart::Image {
        media_type: tqa_types::ImageMediaType::Png,
        data: PNG.to_vec(),
    });
    let mut file = message(Role::Tool, "contents");
    file.tool_call_id = Some("c2".into());
    CompletionRequest {
        messages: vec![
            message(Role::User, "Look."),
            asked,
            shot,
            file,
            message(Role::Assistant, "Seen."),
        ],
        ..CompletionRequest::default()
    }
}

fn body(protocol: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&encode_request(
        &route(protocol, true)?,
        &request(),
    )?)?)
}

// TEST: LLM-U-052
#[test]
fn llm_u_052_messages_puts_the_image_inside_its_tool_result() -> TestResult {
    let body = body("anthropic_messages")?;
    let results = &body["messages"][2]["content"];
    assert_eq!(
        results[0],
        json!({"type": "tool_result", "tool_use_id": "c1", "content": [
            {"type": "text", "text": "Screenshot 2×2."},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw=="}},
        ]})
    );
    assert_eq!(
        results[1],
        json!({"type": "tool_result", "tool_use_id": "c2", "content": "contents"}),
        "a result without an image keeps its plain text"
    );
    Ok(())
}

// TEST: LLM-U-052
#[test]
fn llm_u_052_chat_sends_the_image_after_the_run_of_tool_results() -> TestResult {
    let body = body("openai_chat")?;
    let messages = body["messages"].as_array().ok_or("no messages")?;
    let roles: Vec<&str> = messages
        .iter()
        .filter_map(|message| message["role"].as_str())
        .collect();
    assert_eq!(
        roles,
        ["user", "assistant", "tool", "tool", "user", "assistant"]
    );
    assert_eq!(
        messages[2]["content"],
        json!("Screenshot 2×2."),
        "the result's text alone"
    );
    assert_eq!(
        messages[4]["content"],
        json!([
            {"type": "text", "text": "The image from the result of tool call c1:"},
            {"type": "image_url", "image_url": {"url": PNG_URL}},
        ])
    );
    Ok(())
}

// TEST: LLM-U-052
#[test]
fn llm_u_052_responses_sends_the_image_after_the_run_of_tool_outputs() -> TestResult {
    let body = body("openai_responses")?;
    let input = body["input"].as_array().ok_or("no input")?;
    let kinds: Vec<String> = input
        .iter()
        .map(|item| {
            format!(
                "{}:{}",
                item["type"].as_str().unwrap_or(""),
                item["role"].as_str().unwrap_or("")
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "message:user",
            "function_call:",
            "function_call:",
            "function_call_output:",
            "function_call_output:",
            "message:user",
            "message:assistant",
        ]
    );
    assert_eq!(input[3]["output"], json!("Screenshot 2×2."));
    assert_eq!(
        input[5]["content"],
        json!([
            {"type": "input_text", "text": "The image from the result of tool call c1:"},
            {"type": "input_image", "image_url": PNG_URL},
        ])
    );
    Ok(())
}

// TEST: LLM-U-052
#[test]
fn llm_u_052_a_route_without_images_refuses_one_on_a_tool_result() -> TestResult {
    for protocol in ["anthropic_messages", "openai_chat", "openai_responses"] {
        let refused = encode_request(&route(protocol, false)?, &request());
        assert!(
            matches!(&refused, Err(tqa_llm::LlmError::Protocol(text)) if text.contains("image input")),
            "{protocol}: {refused:?}"
        );
    }
    Ok(())
}
