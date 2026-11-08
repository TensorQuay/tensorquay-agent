use super::*;
use crate::ToolCall;

#[test]
fn llm_u_011_chat_encoder_restores_raw_nonobject_argument_text() -> Result<(), LlmError> {
    for (stored, expected) in [
        (r#""{""#, "{"),
        (r#""[]""#, "[]"),
        (r#"{"path":"file"}"#, r#"{"path":"file"}"#),
    ] {
        let input = Message {
            role: Role::Assistant,
            content: vec![],
            tool_calls: vec![ToolCall {
                id: "call".into(),
                name: "read_file".into(),
                arguments: stored.into(),
            }],
            tool_call_id: None,
            name: None,
        };
        let encoded = message(&input, false)?;
        assert_eq!(
            encoded["tool_calls"][0]["function"]["arguments"],
            json!(expected)
        );
    }
    Ok(())
}

fn vision_route(vision: bool) -> Result<Route, Box<dyn std::error::Error>> {
    let registry = crate::Registry::from_toml(&format!(
        "[[routes]]\nname='r'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='m'\nvision={vision}\n"
    ))?;
    Ok(registry.get("r").ok_or("missing route")?.clone())
}
fn user(content: Vec<ContentPart>) -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message {
            role: Role::User,
            content,
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }],
        ..CompletionRequest::default()
    }
}
fn image(media_type: tqa_types::ImageMediaType) -> ContentPart {
    ContentPart::Image {
        media_type,
        data: b"abc".to_vec(),
    }
}

#[test]
fn llm_c_023_image_parts_encode_as_data_urls_in_part_order()
-> Result<(), Box<dyn std::error::Error>> {
    use tqa_types::ImageMediaType::{Gif, Jpeg, Png, Webp};
    let request = user(vec![
        ContentPart::Text("look".into()),
        image(Png),
        image(Jpeg),
        image(Gif),
        image(Webp),
        ContentPart::Text(" end".into()),
    ]);
    let route = vision_route(true)?;
    let bytes = encode(&route, &request)?;
    let body: Value = serde_json::from_slice(&bytes)?;
    let url = |kind: &str| json!({"type":"image_url","image_url":{"url":format!("data:image/{kind};base64,YWJj")}});
    assert_eq!(
        body["messages"][0]["content"],
        json!([
            {"type":"text","text":"look"},
            url("png"),
            url("jpeg"),
            url("gif"),
            url("webp"),
            {"type":"text","text":" end"}
        ])
    );
    assert_eq!(crate::request_body_len(&route, &request)?, bytes.len());
    Ok(())
}

#[test]
fn llm_c_023_text_only_messages_keep_string_content_bytes() -> Result<(), Box<dyn std::error::Error>>
{
    let request = user(vec![
        ContentPart::Text("hello ".into()),
        ContentPart::Text("world".into()),
    ]);
    for vision in [false, true] {
        let body: Value = serde_json::from_slice(&encode(&vision_route(vision)?, &request)?)?;
        assert_eq!(body["messages"][0]["content"], json!("hello world"));
    }
    Ok(())
}

#[test]
fn llm_c_023_an_image_on_a_non_vision_route_is_refused_before_sending()
-> Result<(), Box<dyn std::error::Error>> {
    let request = user(vec![image(tqa_types::ImageMediaType::Png)]);
    let error = encode(&vision_route(false)?, &request)
        .err()
        .ok_or("image accepted")?;
    assert!(matches!(error, LlmError::Protocol(_)));
    assert!(!format!("{error:?}").contains("YWJj"));
    Ok(())
}

/// Bytes captured from the encoder before `extra_body` objects were deep-merged; Chat requests
/// must not change.
#[test]
fn llm_u_019_chat_request_bytes_are_unchanged_by_the_deep_merge()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = crate::Registry::from_toml(
        "[[routes]]\nname='r'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='m'\n\
         native_tools=true\nparallel_tools=true\nstream_usage=true\nmax_output=64\n\
         reasoning={field='reasoning_effort',allowed=['low','high'],default='low'}\n\
         extra_body={chat_template_kwargs={enable_thinking=true},top_k=20}\n",
    )?;
    let route = registry.get("r").ok_or("missing route")?;
    let mut assistant = Message {
        role: Role::Assistant,
        content: vec![ContentPart::Text("Reading.".into())],
        tool_calls: vec![ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a"}"#.into(),
        }],
        tool_call_id: None,
        name: None,
    };
    assistant.name = Some("agent".into());
    let request = CompletionRequest {
        messages: vec![
            Message {
                role: Role::System,
                content: vec![ContentPart::Text("Be brief.".into())],
                tool_calls: vec![],
                tool_call_id: None,
                name: None,
            },
            assistant,
            Message {
                role: Role::Tool,
                content: vec![ContentPart::Text("body".into())],
                tool_calls: vec![],
                tool_call_id: Some("call_1".into()),
                name: Some("read_file".into()),
            },
        ],
        tools: vec![crate::ToolSpec {
            name: "read_file".into(),
            description: "Read".into(),
            parameters: json!({"type":"object"}),
            strict: false,
        }],
        reasoning_effort: Some("high".into()),
        max_output: None,
        temperature: Some(0.25),
        stop: vec!["END".into()],
        metadata: std::collections::BTreeMap::from([("run".into(), "r1".into())]),
        declare_tools: false,
        // The run travels as a header to the relay only: the exact body below holds none of it (LLM-U-049).
        run_id: tqa_types::RunId::try_from("run-in-header".to_owned()).ok(),
    };
    let encoded = String::from_utf8(encode(route, &request)?)?;
    assert_eq!(
        encoded,
        r#"{"chat_template_kwargs":{"enable_thinking":true},"max_tokens":64,"messages":[{"content":"Be brief.","role":"system"},{"content":"Reading.","name":"agent","role":"assistant","tool_calls":[{"function":{"arguments":"{\"path\":\"a\"}","name":"read_file"},"id":"call_1","type":"function"}]},{"content":"body","name":"read_file","role":"tool","tool_call_id":"call_1"}],"metadata":{"run":"r1"},"model":"m","parallel_tool_calls":true,"reasoning_effort":"high","stop":["END"],"stream":true,"stream_options":{"include_usage":true},"temperature":0.25,"tools":[{"function":{"description":"Read","name":"read_file","parameters":{"type":"object"},"strict":false},"type":"function"}],"top_k":20}"#
    );
    Ok(())
}
