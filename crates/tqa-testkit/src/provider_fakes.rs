//! Provider profiles for [`crate::FakeModelServer`]: the request rules a hosted provider enforces and the error body it
//! answers a violation with, so contract tests see the refusals real providers give (TK-U-016).
//!
//! Every rule names its source: the provider's public API documentation, or a request captured in the field. Rules are
//! deliberately independent of our own request checks (for example `strict_compatible`), so a test against a fake
//! can't pass because both sides share a mistake. When a live run (the manual provider suite) disagrees with a rule,
//! the rule changes and its source names the live evidence.

mod anthropic_messages;
mod deepseek_chat;
mod gemini_openai_compat;
pub use gemini_openai_compat::{THOUGHT_SIGNATURE_PATH, thought_signature};
mod openai_responses;
mod openai_responses_backend;
mod strict;

use serde_json::Value;

/// A hosted provider whose request rules and error shape a fake reproduces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FakeProfile {
    /// The Responses provider as its public API documents it.
    OpenaiResponses,
    /// The hosted Responses backend reached through an account proxy, as observed.
    OpenaiResponsesBackend,
    /// The hosted Messages provider.
    AnthropicMessages,
    /// A hosted Chat provider with a strict deserializer (known roles only).
    DeepseekChat,
    /// A hosted provider reached through its compatible Chat endpoint.
    GeminiOpenaiCompat,
}

/// Where a rule comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// The provider's public API documentation.
    Documented(&'static str),
    /// A request and refusal captured in the field.
    Observed(&'static str),
}

/// A broken rule, as the provider words it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Violation {
    /// The provider's error message.
    pub message: String,
    /// The request field the provider names, when it names one.
    pub param: Option<String>,
    /// The provider's error code, when it gives one.
    pub code: Option<&'static str>,
}

/// One request rule of a profile.
pub struct Rule {
    /// A stable name, used by the self-tests.
    pub name: &'static str,
    /// Where the rule comes from.
    pub source: Source,
    /// The HTTP status of the refusal.
    pub status: u16,
    pub(super) check: fn(&Value) -> Option<Violation>,
}

/// The fake's answer to a request that breaks a rule (or uses another protocol's path).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// The broken rule's name (`not_found` for a wrong path).
    pub rule: &'static str,
    /// The HTTP status.
    pub status: u16,
    /// The JSON error body, in the provider's shape.
    pub body: String,
}

impl FakeProfile {
    /// The request path this provider serves, under the fake's `/v1` prefix.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::OpenaiResponses => openai_responses::PATH,
            Self::OpenaiResponsesBackend => openai_responses_backend::PATH,
            Self::AnthropicMessages => anthropic_messages::PATH,
            Self::DeepseekChat => deepseek_chat::PATH,
            Self::GeminiOpenaiCompat => gemini_openai_compat::PATH,
        }
    }
    /// The profile's rules, checked in order; the first broken one answers.
    #[must_use]
    pub const fn rules(self) -> &'static [Rule] {
        match self {
            Self::OpenaiResponses => openai_responses::RULES,
            Self::OpenaiResponsesBackend => openai_responses_backend::RULES,
            Self::AnthropicMessages => anthropic_messages::RULES,
            Self::DeepseekChat => deepseek_chat::RULES,
            Self::GeminiOpenaiCompat => gemini_openai_compat::RULES,
        }
    }
    /// The refusal for a request, or `None` when the provider would serve it.
    #[must_use]
    pub fn check(self, path: &str, body: &Value) -> Option<Refusal> {
        if path != self.path() {
            return Some(self.refusal(
                "not_found",
                404,
                &Violation::new(format!("Invalid URL (POST {path})")),
            ));
        }
        self.rules().iter().find_map(|rule| {
            (rule.check)(body).map(|violation| self.refusal(rule.name, rule.status, &violation))
        })
    }
    /// The message field of an error body in this provider's shape.
    #[must_use]
    pub fn error_message(self, body: &Value) -> Option<String> {
        let message = match self {
            Self::OpenaiResponses | Self::AnthropicMessages | Self::DeepseekChat => {
                &body["error"]["message"]
            }
            Self::GeminiOpenaiCompat => &body[0]["error"]["message"],
            Self::OpenaiResponsesBackend => &body["detail"],
        };
        message.as_str().map(str::to_owned)
    }
    /// One frame of a successful Chat stream as this provider streams it. The compatible endpoint sends tool-call
    /// deltas without `index`, as observed live, and signs each call on the piece that carries its id; other lines,
    /// and other providers' frames, pass unchanged.
    #[must_use]
    pub fn restyle_chat_frame(self, frame: &[u8]) -> Vec<u8> {
        if self != Self::GeminiOpenaiCompat {
            return frame.to_vec();
        }
        let Ok(text) = std::str::from_utf8(frame) else {
            return frame.to_vec();
        };
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            out.push_str(&without_call_index(line));
        }
        out.into_bytes()
    }
    fn refusal(self, rule: &'static str, status: u16, violation: &Violation) -> Refusal {
        let body = match self {
            Self::OpenaiResponses => openai_responses::error_body(status, violation),
            Self::OpenaiResponsesBackend => openai_responses_backend::error_body(status, violation),
            Self::AnthropicMessages => anthropic_messages::error_body(status, violation),
            Self::DeepseekChat => deepseek_chat::error_body(status, violation),
            Self::GeminiOpenaiCompat => gemini_openai_compat::error_body(status, violation),
        };
        Refusal {
            rule,
            status,
            body: body.to_string(),
        }
    }
}

impl Violation {
    /// A violation with only a message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            param: None,
            code: None,
        }
    }
}

/// One SSE line with the `index` of each tool-call delta removed and each call signed; any other line as it is.
fn without_call_index(line: &str) -> String {
    let Some(data) = line.strip_prefix("data: ") else {
        return line.to_owned();
    };
    let (json, end) = data.split_at(data.trim_end_matches(['\r', '\n']).len());
    let Ok(mut chunk) = serde_json::from_str::<Value>(json) else {
        return line.to_owned();
    };
    // `get_mut`, never indexing: indexing a missing key would insert a null.
    let choices = chunk.get_mut("choices").and_then(Value::as_array_mut);
    let calls = choices.into_iter().flatten().filter_map(|choice| {
        choice
            .get_mut("delta")
            .and_then(|delta| delta.get_mut("tool_calls"))
            .and_then(Value::as_array_mut)
    });
    for call in calls.flatten().filter_map(Value::as_object_mut) {
        call.remove("index");
        gemini_openai_compat::sign(call);
    }
    format!("data: {chunk}{end}")
}

/// The array field's items, or none.
pub(super) fn items<'a>(body: &'a Value, field: &str) -> impl Iterator<Item = &'a Value> {
    body[field].as_array().into_iter().flatten()
}

pub(super) fn valid_tool_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
