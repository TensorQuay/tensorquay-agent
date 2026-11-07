//! The same synthetic scripts rendered as a Responses stream: items, deltas and one terminal event.

use super::{Frame, MAX_SCRIPT_EVENTS, frames};
use crate::{MalformedSse, Script, Scripted};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io, time::Duration};
use tqa_llm::{FinishReason, Usage};

/// The item currently receiving text or reasoning deltas.
#[derive(Clone, Copy, PartialEq)]
enum Open {
    Message(u64),
    Reasoning(u64),
}

/// An explicitly indexed call, closed just before the terminal event.
struct Explicit {
    output: u64,
    call_id: String,
    name: String,
    arguments: String,
}

struct Compiler {
    records: Vec<(String, Duration)>,
    delay: Duration,
    usage: Option<Usage>,
    finish: FinishReason,
    next_output: u64,
    next_call: u32,
    call_prefix: String,
    cut: Option<usize>,
    variant: Option<MalformedSse>,
    open: Option<Open>,
    explicit: BTreeMap<u32, Explicit>,
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

    fn output(&mut self) -> u64 {
        self.next_output += 1;
        self.next_output - 1
    }

    fn close(&mut self) -> io::Result<()> {
        match self.open.take() {
            Some(Open::Message(index)) => self.push(&json!({"type":"response.output_item.done",
                "output_index":index,"item":{"type":"message","id":format!("msg_{index}"),"role":"assistant"}})),
            Some(Open::Reasoning(index)) => self.push(&json!({"type":"response.output_item.done",
                "output_index":index,"item":{"type":"reasoning","id":format!("rs_{index}")}})),
            None => Ok(()),
        }
    }

    /// Opens a message or reasoning item unless one of that kind is already receiving deltas.
    fn delta(&mut self, reasoning: bool, text: &str) -> io::Result<()> {
        let index = match self.open {
            Some(Open::Message(index)) if !reasoning => index,
            Some(Open::Reasoning(index)) if reasoning => index,
            _ => {
                self.close()?;
                let index = self.output();
                let item = if reasoning {
                    json!({"type":"reasoning","id":format!("rs_{index}")})
                } else {
                    json!({"type":"message","id":format!("msg_{index}"),"role":"assistant","content":[]})
                };
                self.push(
                    &json!({"type":"response.output_item.added","output_index":index,"item":item}),
                )?;
                self.open = Some(if reasoning {
                    Open::Reasoning(index)
                } else {
                    Open::Message(index)
                });
                index
            }
        };
        let kind = if reasoning {
            "response.reasoning_text.delta"
        } else {
            "response.output_text.delta"
        };
        self.push(&json!({"type":kind,"output_index":index,"content_index":0,"delta":text}))
    }

    fn call_done(
        &mut self,
        output: u64,
        call_id: &str,
        name: &str,
        arguments: &str,
    ) -> io::Result<()> {
        self.push(&json!({"type":"response.function_call_arguments.done","output_index":output,"arguments":arguments}))?;
        self.push(
            &json!({"type":"response.output_item.done","output_index":output,
            "item":{"type":"function_call","call_id":call_id,"name":name,"arguments":arguments}}),
        )
    }

    fn tool(&mut self, name: &str, arguments: &str, parts: usize) -> io::Result<()> {
        if parts == 0 || parts > MAX_SCRIPT_EVENTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid tool fragment count",
            ));
        }
        self.close()?;
        let output = self.output();
        let call_id = format!("{}-{}", self.call_prefix, self.next_call);
        self.push(&json!({"type":"response.output_item.added","output_index":output,
            "item":{"type":"function_call","id":format!("fc_{output}"),"call_id":call_id,"name":name,"arguments":""}}))?;
        let scalars: Vec<char> = arguments.chars().collect();
        for part in 0..parts {
            let start = scalars.len() * part / parts;
            let end = scalars.len() * (part + 1) / parts;
            let delta: String = scalars[start..end].iter().collect();
            self.push(&json!({"type":"response.function_call_arguments.delta","output_index":output,"delta":delta}))?;
        }
        self.call_done(output, &call_id, name, arguments)?;
        self.next_call += 1;
        self.finish = FinishReason::ToolCalls;
        Ok(())
    }

    fn indexed(
        &mut self,
        index: u32,
        id: Option<&str>,
        name: Option<&str>,
        delta: &str,
    ) -> io::Result<()> {
        if !self.explicit.contains_key(&index) {
            self.close()?;
            let output = self.output();
            let call = Explicit {
                output,
                call_id: id.map_or_else(|| format!("{}-{index}", self.call_prefix), str::to_owned),
                name: name.unwrap_or("tool").to_owned(),
                arguments: String::new(),
            };
            self.push(&json!({"type":"response.output_item.added","output_index":output,
                "item":{"type":"function_call","id":format!("fc_{output}"),"call_id":call.call_id,"name":call.name,"arguments":""}}))?;
            self.explicit.insert(index, call);
        }
        let Some(call) = self.explicit.get_mut(&index) else {
            return Err(too_many());
        };
        call.arguments.push_str(delta);
        let output = call.output;
        self.finish = FinishReason::ToolCalls;
        self.push(&json!({"type":"response.function_call_arguments.delta","output_index":output,"delta":delta}))
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
            } => self.indexed(*index, id.as_deref(), name.as_deref(), arguments_delta)?,
            Scripted::Usage(value) => self.usage = Some(value.clone()),
            Scripted::CutAfter(count) => self.cut = Some(*count),
            Scripted::MalformedSse(variant) => self.variant = Some(*variant),
            Scripted::Delay(duration) => self.delay = self.delay.saturating_add(*duration),
            Scripted::Finish(reason) => self.finish = reason.clone(),
            Scripted::RawJson(value) => {
                if self.records.len() >= MAX_SCRIPT_EVENTS {
                    return Err(too_many());
                }
                self.records
                    .push((value.clone(), std::mem::take(&mut self.delay)));
            }
            Scripted::Status { .. } | Scripted::Header(..) => {}
        }
        Ok(())
    }

    fn terminal(&mut self) -> io::Result<()> {
        self.close()?;
        for call in std::mem::take(&mut self.explicit).into_values() {
            self.call_done(call.output, &call.call_id, &call.name, &call.arguments)?;
        }
        let mut response = json!({"object":"response","output":[]});
        if let Some(usage) = &self.usage {
            let mut value =
                json!({"input_tokens":usage.prompt_tokens,"output_tokens":usage.completion_tokens});
            if let Some(cached) = usage.cached_tokens {
                value["input_tokens_details"] = json!({"cached_tokens":cached});
            }
            if let Some(reasoning) = usage.reasoning_tokens {
                value["output_tokens_details"] = json!({"reasoning_tokens":reasoning});
            }
            response["usage"] = value;
        }
        let reason = match &self.finish {
            FinishReason::Length => Some("max_output_tokens"),
            FinishReason::ContentFilter => Some("content_filter"),
            FinishReason::Other(reason) => Some(reason.as_str()),
            _ => None,
        };
        let kind = if let Some(reason) = reason {
            response["status"] = json!("incomplete");
            response["incomplete_details"] = json!({"reason":reason});
            "response.incomplete"
        } else {
            response["status"] = json!("completed");
            "response.completed"
        };
        self.push(&json!({"type":kind,"response":response}))
    }
}

pub(super) fn compile(script: &Script, call_prefix: String) -> io::Result<Vec<Frame>> {
    if script.events.len() > MAX_SCRIPT_EVENTS {
        return Err(too_many());
    }
    let mut compiler = Compiler {
        records: Vec::new(),
        delay: Duration::ZERO,
        usage: None,
        finish: FinishReason::Stop,
        next_output: 0,
        next_call: 0,
        call_prefix,
        cut: None,
        variant: None,
        open: None,
        explicit: BTreeMap::new(),
    };
    for event in &script.events {
        compiler.event(event)?;
    }
    if let Some(count) = compiler.cut {
        compiler.records.truncate(count);
    } else {
        compiler.terminal()?;
    }
    let created = json!({"type":"response.created","response":{"object":"response","status":"in_progress","output":[]}});
    compiler
        .records
        .insert(0, (created.to_string(), Duration::ZERO));
    frames(compiler.records, compiler.variant, true)
}
