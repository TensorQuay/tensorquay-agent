//! The same synthetic scripts rendered as a Messages stream: content blocks between one
//! `message_start` and one `message_stop`.

use super::{Frame, MAX_SCRIPT_EVENTS, frames};
use crate::{Script, Scripted};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io, time::Duration};
use tqa_llm::{FinishReason, Usage};

/// The block currently receiving deltas; a thinking block closes with a synthetic signature.
#[derive(Clone, Copy, PartialEq)]
enum Open {
    Text(u64),
    Thinking(u64),
}

struct Compiler {
    records: Vec<(String, Duration)>,
    delay: Duration,
    finish: FinishReason,
    next_block: u64,
    next_call: u32,
    call_prefix: String,
    open: Option<Open>,
    /// Explicitly indexed calls; the protocol never interleaves blocks, so they stream whole at the end.
    explicit: BTreeMap<u32, (String, String, Vec<String>)>,
}

fn too_many() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "too many script events")
}

impl Compiler {
    fn push(&mut self, value: &Value) -> io::Result<()> {
        if self.records.len() >= MAX_SCRIPT_EVENTS {
            return Err(too_many());
        }
        self.records
            .push((value.to_string(), std::mem::take(&mut self.delay)));
        Ok(())
    }

    fn block(&mut self, block: &Value) -> io::Result<u64> {
        self.next_block += 1;
        let index = self.next_block - 1;
        self.push(&json!({"type":"content_block_start","index":index,"content_block":block}))?;
        Ok(index)
    }

    fn close(&mut self) -> io::Result<()> {
        let index = match self.open.take() {
            Some(Open::Thinking(index)) => {
                let signature =
                    json!({"type":"signature_delta","signature":format!("sig-{index}")});
                self.push(&json!({"type":"content_block_delta","index":index,"delta":signature}))?;
                index
            }
            Some(Open::Text(index)) => index,
            None => return Ok(()),
        };
        self.push(&json!({"type":"content_block_stop","index":index}))
    }

    fn delta(&mut self, thinking: bool, text: &str) -> io::Result<()> {
        let index = match self.open {
            Some(Open::Text(index)) if !thinking => index,
            Some(Open::Thinking(index)) if thinking => index,
            _ => {
                self.close()?;
                let (block, open): (Value, fn(u64) -> Open) = if thinking {
                    (
                        json!({"type":"thinking","thinking":"","signature":""}),
                        Open::Thinking,
                    )
                } else {
                    (json!({"type":"text","text":""}), Open::Text)
                };
                let index = self.block(&block)?;
                self.open = Some(open(index));
                index
            }
        };
        let delta = if thinking {
            json!({"type":"thinking_delta","thinking":text})
        } else {
            json!({"type":"text_delta","text":text})
        };
        self.push(&json!({"type":"content_block_delta","index":index,"delta":delta}))
    }

    fn call(&mut self, id: &str, name: &str, fragments: &[String]) -> io::Result<()> {
        self.close()?;
        let index = self.block(&json!({"type":"tool_use","id":id,"name":name,"input":{}}))?;
        for fragment in fragments {
            let delta = json!({"type":"input_json_delta","partial_json":fragment});
            self.push(&json!({"type":"content_block_delta","index":index,"delta":delta}))?;
        }
        self.finish = FinishReason::ToolCalls;
        self.push(&json!({"type":"content_block_stop","index":index}))
    }

    fn tool(&mut self, name: &str, arguments: &str, parts: usize) -> io::Result<()> {
        if parts == 0 || parts > MAX_SCRIPT_EVENTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid tool fragment count",
            ));
        }
        let scalars: Vec<char> = arguments.chars().collect();
        let fragments: Vec<String> = (0..parts)
            .map(|part| {
                let start = scalars.len() * part / parts;
                let end = scalars.len() * (part + 1) / parts;
                scalars[start..end].iter().collect()
            })
            .collect();
        let id = format!("{}-{}", self.call_prefix, self.next_call);
        self.next_call += 1;
        self.call(&id, name, &fragments)
    }

    fn event(&mut self, event: &Scripted) -> io::Result<()> {
        match event {
            Scripted::Text(text) => self.delta(false, text)?,
            Scripted::Reasoning(text) => self.delta(true, text)?,
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
                let prefix = &self.call_prefix;
                let call = self.explicit.entry(*index).or_insert_with(|| {
                    let id = id.clone().unwrap_or_else(|| format!("{prefix}-{index}"));
                    (
                        id,
                        name.clone().unwrap_or_else(|| "tool".into()),
                        Vec::new(),
                    )
                });
                call.2.push(arguments_delta.clone());
                self.finish = FinishReason::ToolCalls;
            }
            Scripted::Delay(duration) => self.delay = self.delay.saturating_add(*duration),
            Scripted::Finish(reason) => self.finish = reason.clone(),
            Scripted::RawJson(value) => {
                if self.records.len() >= MAX_SCRIPT_EVENTS {
                    return Err(too_many());
                }
                self.records
                    .push((value.clone(), std::mem::take(&mut self.delay)));
            }
            Scripted::Usage(_)
            | Scripted::CutAfter(_)
            | Scripted::MalformedSse(_)
            | Scripted::Status { .. }
            | Scripted::Header(..) => {}
        }
        Ok(())
    }

    fn terminal(&mut self, usage: Option<&Usage>) -> io::Result<()> {
        self.close()?;
        for (id, name, fragments) in std::mem::take(&mut self.explicit).into_values() {
            self.call(&id, &name, &fragments)?;
        }
        let reason = match &self.finish {
            FinishReason::ToolCalls => "tool_use".to_owned(),
            FinishReason::Length => "max_tokens".to_owned(),
            FinishReason::ContentFilter => "refusal".to_owned(),
            FinishReason::Other(reason) => reason.clone(),
            _ => "end_turn".to_owned(),
        };
        let mut delta =
            json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":null}});
        if let Some(usage) = usage {
            delta["usage"] = json!({"output_tokens":usage.completion_tokens});
        }
        self.push(&delta)?;
        self.push(&json!({"type":"message_stop"}))
    }
}

/// The input side of the usage, as `message_start` reports it: uncached input and cache reads are
/// separate there, while `Usage::prompt_tokens` is the whole prompt.
fn start(usage: Option<&Usage>) -> Value {
    let mut message = json!({"id":"msg_fake","type":"message","role":"assistant","content":[],"stop_reason":null});
    if let Some(usage) = usage {
        let cached = usage.cached_tokens.unwrap_or(0);
        let mut value =
            json!({"input_tokens":usage.prompt_tokens.saturating_sub(cached),"output_tokens":0});
        if let Some(cached) = usage.cached_tokens {
            value["cache_read_input_tokens"] = json!(cached);
        }
        message["usage"] = value;
    }
    json!({"type":"message_start","message":message})
}

pub(super) fn compile(script: &Script, call_prefix: String) -> io::Result<Vec<Frame>> {
    if script.events.len() > MAX_SCRIPT_EVENTS {
        return Err(too_many());
    }
    let find = |pick: fn(&Scripted) -> bool| script.events.iter().rev().find(|event| pick(event));
    let usage = match find(|event| matches!(event, Scripted::Usage(_))) {
        Some(Scripted::Usage(usage)) => Some(usage),
        _ => None,
    };
    let mut compiler = Compiler {
        records: Vec::new(),
        delay: Duration::ZERO,
        finish: FinishReason::Stop,
        next_block: 0,
        next_call: 0,
        call_prefix,
        open: None,
        explicit: BTreeMap::new(),
    };
    for event in &script.events {
        compiler.event(event)?;
    }
    let cut = script.events.iter().rev().find_map(|event| match event {
        Scripted::CutAfter(count) => Some(*count),
        _ => None,
    });
    let variant = script.events.iter().rev().find_map(|event| match event {
        Scripted::MalformedSse(variant) => Some(*variant),
        _ => None,
    });
    if let Some(count) = cut {
        compiler.records.truncate(count);
    } else {
        compiler.terminal(usage)?;
    }
    compiler
        .records
        .insert(0, (start(usage).to_string(), Duration::ZERO));
    frames(compiler.records, variant, true)
}
