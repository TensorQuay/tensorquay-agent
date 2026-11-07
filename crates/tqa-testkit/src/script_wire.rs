//! Independent synthetic wire generation for the local model server.

use crate::{MalformedSse, Script, Scripted};
use bytes::Bytes;
use serde_json::{Value, json};
use std::{io, time::Duration};
use tqa_llm::{FinishReason, Usage};

mod messages;
mod responses;

pub(crate) const MAX_SCRIPT_EVENTS: usize = 16_384;
pub(crate) struct Frame {
    pub bytes: Bytes,
    pub delay: Duration,
}

fn chunk(delta: &Value, finish: &Value) -> String {
    json!({"choices":[{"index":0,"delta":delta,"finish_reason":finish}]}).to_string()
}

fn usage(usage: &Usage) -> String {
    let mut value =
        json!({"prompt_tokens":usage.prompt_tokens,"completion_tokens":usage.completion_tokens});
    if let Some(cached) = usage.cached_tokens {
        value["prompt_tokens_details"] = json!({"cached_tokens":cached});
    }
    if let Some(reasoning) = usage.reasoning_tokens {
        value["completion_tokens_details"] = json!({"reasoning_tokens":reasoning});
    }
    json!({"choices":[],"usage":value}).to_string()
}

fn finish(reason: &FinishReason) -> &str {
    match reason {
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::Length => "length",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Other(reason) => reason,
        _ => "stop",
    }
}

struct Compiler {
    records: Vec<(String, Duration)>,
    usages: Vec<String>,
    delay: Duration,
    finish: FinishReason,
    next_call: u32,
    call_prefix: String,
    cut: Option<usize>,
    variant: Option<MalformedSse>,
}

impl Compiler {
    fn push(&mut self, value: String) -> io::Result<()> {
        if self.records.len() >= MAX_SCRIPT_EVENTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "too many script events",
            ));
        }
        self.records.push((value, std::mem::take(&mut self.delay)));
        Ok(())
    }

    fn tool(&mut self, name: &str, arguments: &str, parts: usize) -> io::Result<()> {
        if parts == 0 || parts > MAX_SCRIPT_EVENTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid tool fragment count",
            ));
        }
        let scalars: Vec<char> = arguments.chars().collect();
        for part in 0..parts {
            let start = scalars.len() * part / parts;
            let end = scalars.len() * (part + 1) / parts;
            let mut call = json!({"index":self.next_call,"function":{"arguments":scalars[start..end].iter().collect::<String>()}});
            if part == 0 {
                call["id"] = json!(format!("{}-{}", self.call_prefix, self.next_call));
                call["type"] = json!("function");
                call["function"]["name"] = json!(name);
            }
            self.push(chunk(&json!({"tool_calls":[call]}), &Value::Null))?;
        }
        self.next_call += 1;
        self.finish = FinishReason::ToolCalls;
        Ok(())
    }

    fn event(&mut self, event: &Scripted) -> io::Result<()> {
        match event {
            Scripted::Text(text) => self.push(chunk(&json!({"content":text}), &Value::Null))?,
            Scripted::Reasoning(text) => {
                self.push(chunk(&json!({"reasoning_content":text}), &Value::Null))?;
            }
            Scripted::ToolCall {
                name,
                arguments_json,
                split_into,
            } => self.tool(name, arguments_json, *split_into)?,
            Scripted::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let mut call = json!({"index":index,"function":{"arguments":arguments_delta}});
                if let Some(id) = id {
                    call["id"] = json!(id);
                }
                if let Some(name) = name {
                    call["function"]["name"] = json!(name);
                }
                self.push(chunk(&json!({"tool_calls":[call]}), &Value::Null))?;
                self.finish = FinishReason::ToolCalls;
            }
            Scripted::Usage(value) => self.usages.push(usage(value)),
            Scripted::CutAfter(count) => self.cut = Some(*count),
            Scripted::MalformedSse(variant) => self.variant = Some(*variant),
            Scripted::Delay(duration) => self.delay = self.delay.saturating_add(*duration),
            Scripted::Finish(reason) => self.finish = reason.clone(),
            Scripted::RawJson(value) => self.push(value.clone())?,
            Scripted::Status { .. } | Scripted::Header(..) => {}
        }
        Ok(())
    }
}

pub(crate) fn render(script: &Script) -> io::Result<Vec<Frame>> {
    compile(script, "call".into())
}

pub(crate) fn render_turn(script: &Script, turn: usize) -> io::Result<Vec<Frame>> {
    compile(script, format!("call-{turn}"))
}

/// The same script as a Responses stream (`POST /responses`).
pub(crate) fn render_responses(script: &Script) -> io::Result<Vec<Frame>> {
    responses::compile(script, "call".into())
}

pub(crate) fn render_responses_turn(script: &Script, turn: usize) -> io::Result<Vec<Frame>> {
    responses::compile(script, format!("call-{turn}"))
}

/// The same script as a Messages stream (`POST /messages`).
pub(crate) fn render_messages(script: &Script) -> io::Result<Vec<Frame>> {
    messages::compile(script, "call".into())
}

pub(crate) fn render_messages_turn(script: &Script, turn: usize) -> io::Result<Vec<Frame>> {
    messages::compile(script, format!("call-{turn}"))
}

fn compile(script: &Script, call_prefix: String) -> io::Result<Vec<Frame>> {
    if script.events.len() > MAX_SCRIPT_EVENTS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many script entries",
        ));
    }
    let mut compiler = Compiler {
        records: Vec::new(),
        usages: Vec::new(),
        delay: Duration::ZERO,
        finish: FinishReason::Stop,
        next_call: 0,
        call_prefix,
        cut: None,
        variant: None,
    };
    for event in &script.events {
        compiler.event(event)?;
    }
    if let Some(count) = compiler.cut {
        compiler.records.truncate(count);
    } else {
        compiler.push(chunk(&json!({}), &json!(finish(&compiler.finish))))?;
        for value in std::mem::take(&mut compiler.usages) {
            compiler.push(value)?;
        }
        if compiler.variant != Some(MalformedSse::MissingDone) {
            compiler.push("[DONE]".into())?;
        }
    }
    frames(compiler.records, compiler.variant, false)
}

/// Frames records as SSE, optionally with an `event:` line naming each record's `type`.
pub(crate) fn frames(
    records: Vec<(String, Duration)>,
    variant: Option<MalformedSse>,
    event_lines: bool,
) -> io::Result<Vec<Frame>> {
    let mut frames = Vec::new();
    for (record, delay) in records {
        let kind = event_lines
            .then(|| serde_json::from_str::<Value>(&record).ok())
            .flatten()
            .and_then(|value| value["type"].as_str().map(str::to_owned));
        let mut text = match kind {
            Some(kind) => format!("event: {kind}\ndata: {record}\n\n"),
            None => format!("data: {record}\n\n"),
        };
        if variant == Some(MalformedSse::CrLf) {
            text = text.replace('\n', "\r\n");
        }
        if variant == Some(MalformedSse::Comments) {
            text = format!(": keep-alive\n\n{text}\n");
        }
        let bytes = Bytes::from(text);
        let parts = match variant {
            Some(MalformedSse::SplitUtf8) => (0..bytes.len())
                .map(|index| bytes.slice(index..=index))
                .collect(),
            Some(MalformedSse::SplitData) => vec![bytes.slice(..2), bytes.slice(2..)],
            _ => vec![bytes],
        };
        for (index, bytes) in parts.into_iter().enumerate() {
            if frames.len() >= MAX_SCRIPT_EVENTS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "too many wire fragments",
                ));
            }
            frames.push(Frame {
                bytes,
                delay: if index == 0 { delay } else { Duration::ZERO },
            });
        }
    }
    Ok(frames)
}
