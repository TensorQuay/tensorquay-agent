//! Validated model-route data, independent of a provider implementation.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

/// A wire protocol selected explicitly by the registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Protocol {
    /// Streaming chat completions.
    #[serde(rename = "openai_chat")]
    OpenAiChat,
    /// Item-based streaming responses, sent statelessly from our own history.
    #[serde(rename = "openai_responses")]
    OpenAiResponses,
    /// Message-block completions, sent statelessly from our own history.
    AnthropicMessages,
}

/// Exactly one credential placement, selected by configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", try_from = "AuthConfig")]
#[non_exhaustive]
pub enum Auth {
    /// Send no credential.
    #[default]
    None,
    /// Send the credential as an authorization bearer value.
    Bearer,
    /// Send the credential in one configured HTTP header.
    Header {
        /// The HTTP header name, validated at registry load.
        name: String,
    },
    /// Send the credential in one configured query parameter.
    Query {
        /// The query parameter name.
        name: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum AuthConfig {
    None {},
    Bearer {},
    Header { name: String },
    Query { name: String },
}

impl From<AuthConfig> for Auth {
    fn from(value: AuthConfig) -> Self {
        match value {
            AuthConfig::None {} => Self::None,
            AuthConfig::Bearer {} => Self::Bearer,
            AuthConfig::Header { name } => Self::Header { name },
            AuthConfig::Query { name } => Self::Query { name },
        }
    }
}

/// The request field and allowed values for reasoning effort.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reasoning {
    /// The Chat request field for reasoning effort. Responses routes always send
    /// `reasoning.effort` and Messages routes a thinking budget; both ignore this field.
    pub field: String,
    /// The accepted effort values.
    pub allowed: Vec<String>,
    /// The value used when a request does not choose one.
    pub default: String,
    /// The thinking token budget of each allowed value; required on protocols that take a budget
    /// instead of an effort, and refused on the others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<BTreeMap<String, u32>>,
}

/// A declared caching capability: the wire enum routes carry to the runtime (revision 7), one definition.
pub use tqa_types::CacheHint;

/// Declared token limits; missing context capacity remains a guess.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Configured context-window tokens, when known.
    pub context_window: Option<u32>,
    /// Configured maximum output tokens, when known.
    pub max_output: Option<u32>,
    /// At most this many requests in flight on the route, streams included; `None` means unlimited.
    pub concurrency: Option<u32>,
    /// At most this many requests started per rolling minute; `None` means unlimited.
    pub requests_per_minute: Option<u32>,
}

impl Limits {
    /// Returns true only when the context window is configured.
    #[must_use]
    pub const fn derived(&self) -> bool {
        self.context_window.is_some()
    }
}

/// A route read from a validated registry; no credential is stored here.
#[derive(Clone, Debug, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent registry capabilities preserve the approved flat route contract."
)]
pub struct Route {
    /// The logical route name.
    pub name: String,
    /// The selected wire protocol.
    pub protocol: Protocol,
    /// The endpoint prefix, including its API version path when applicable.
    pub base_url: String,
    /// The server's model identifier.
    pub model: String,
    /// The configured credential placement.
    pub auth: Auth,
    /// A secret-store reference, never the credential itself.
    pub key_ref: Option<String>,
    /// Declared token limits.
    pub limits: Limits,
    /// Whether structured tool calls are supported; defaults to true in TOML.
    pub native_tools: bool,
    /// Whether parallel tool calls are supported.
    pub parallel_tools: bool,
    /// Whether image inputs are supported by the route.
    pub vision: bool,
    /// Optional reasoning-effort mapping.
    pub reasoning: Option<Reasoning>,
    /// Declared cache behavior.
    pub cache_hint: CacheHint,
    /// Whether a Chat request asks for usage in the stream's last event: on unless set to false, and false keeps
    /// `stream_options` out of the request, for a server that refuses it. Read it through [`Route::stream_usage`].
    pub stream_usage: Option<bool>,
    /// Additional non-credential HTTP headers, in deterministic order.
    pub extra_headers: BTreeMap<String, String>,
    /// Additional non-reserved JSON request fields, in deterministic order.
    pub extra_body: BTreeMap<String, serde_json::Value>,
    /// Explicitly named adapter workarounds; none are currently implemented.
    pub quirks: Vec<String>,
    /// Admin-declared routes a run may fall back to, in order (ADR-0025).
    pub fallbacks: Vec<String>,
    /// When tools go out with `strict: true` (`auto`, `on` or `off`); absent is `auto`. See [`Route::strict_tools`].
    pub strict: Option<String>,
    /// Which schema keywords tools keep (`full` or `basic`); absent is `full`. See [`Route::schema_keywords`].
    pub schema_keywords: Option<String>,
    /// Where the merged instructions go; absent is the protocol's default. See [`Route::instructions_role`].
    pub instructions_role: Option<String>,
    /// How long to wait for the model's first byte, 5 to 900 seconds; absent, the default for where the model runs. See
    /// [`Route::first_response`].
    pub first_response_secs: Option<u32>,
}

/// The first-response bound of a local route without its own setting: a model server on the same machine, on CPU,
/// reads a long prompt for a minute or more before it answers.
pub const FIRST_RESPONSE_LOCAL: Duration = Duration::from_mins(5);
/// The first-response bound of any other route without its own setting.
pub const FIRST_RESPONSE_REMOTE: Duration = Duration::from_mins(1);
/// The shortest and longest first-response bound a route may set, in seconds.
pub const FIRST_RESPONSE_RANGE: std::ops::RangeInclusive<u32> = 5..=900;

impl Route {
    /// Whether a Chat request asks for usage in the stream: always, unless the route sets `stream_usage = false`
    /// (a server that refuses `stream_options`). Until the next release's change a stored `false` was the old default
    /// and was never applied to runs; migration 0085 turned those into `true`.
    #[must_use]
    pub fn stream_usage(&self) -> bool {
        self.stream_usage != Some(false)
    }

    /// How long to wait for the model's first byte: the route's setting, else the default for a `local` route (the
    /// gateway decides which routes are local) or any other.
    #[must_use]
    pub fn first_response(&self, local: bool) -> Duration {
        self.first_response_secs.map_or(
            if local {
                FIRST_RESPONSE_LOCAL
            } else {
                FIRST_RESPONSE_REMOTE
            },
            |secs| Duration::from_secs(u64::from(secs)),
        )
    }

    /// The route's strict setting; a validated route always has a documented value.
    #[must_use]
    pub fn strict_tools(&self) -> crate::tool_schemas::StrictTools {
        crate::tool_schemas::StrictTools::parse(self.strict.as_deref()).unwrap_or_default()
    }

    /// The route's instructions role; a validated route always has a value its protocol accepts.
    #[must_use]
    pub fn instructions_role(&self) -> crate::instructions_role::InstructionsRole {
        crate::instructions_role::InstructionsRole::parse(
            self.protocol,
            self.instructions_role.as_deref(),
        )
        .unwrap_or_else(|| crate::instructions_role::InstructionsRole::default_for(self.protocol))
    }

    /// The route's schema-keywords setting; a validated route always has a documented value.
    #[must_use]
    pub fn schema_keywords(&self) -> crate::tool_schemas::SchemaKeywords {
        crate::tool_schemas::SchemaKeywords::parse(self.schema_keywords.as_deref())
            .unwrap_or_default()
    }
}

/// One route exactly as `models.toml` declares it, and as a stored route record holds it (JSON with the same
/// fields). Unvalidated: [`crate::Registry::from_toml`] and [`crate::Registry::from_configs`] validate it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "The exact TOML contract declares independent capability flags."
)]
pub struct RouteConfig {
    /// The logical route name users pick.
    pub name: String,
    /// The name people see, from the file: the relay never reads it, and the gateway stores it
    /// beside the route record, never in it. Unset, it is never written, so an existing record is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The wire protocol.
    pub protocol: Protocol,
    /// The endpoint prefix, including its API version path.
    pub base_url: String,
    /// The server's model identifier.
    pub model: String,
    /// The credential placement; `none` when omitted.
    #[serde(default)]
    pub auth: Auth,
    /// A secret-store reference, never the credential itself.
    pub key_ref: Option<String>,
    /// The context window in tokens, when known.
    pub context_window: Option<u32>,
    /// The output limit in tokens, when known.
    pub max_output: Option<u32>,
    /// At most this many requests in flight on the route, across every conversation (the relay's limiter).
    pub concurrency: Option<u32>,
    /// At most this many requests started per rolling minute on the route (the relay's limiter).
    pub requests_per_minute: Option<u32>,
    /// Whether structured tool calls are supported; true when omitted.
    #[serde(default = "enabled")]
    pub native_tools: bool,
    /// Whether several calls may come in one response.
    #[serde(default)]
    pub parallel_tools: bool,
    /// Whether image input is accepted.
    #[serde(default)]
    pub vision: bool,
    /// The reasoning mapping, when the route has one.
    pub reasoning: Option<Reasoning>,
    /// The declared caching capability.
    #[serde(default)]
    pub cache_hint: CacheHint,
    /// Whether a Chat request asks for usage in the stream; absent means yes, and `false` keeps `stream_options` out
    /// of the request for a server that refuses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_usage: Option<bool>,
    /// Non-credential headers merged into every request.
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
    /// Non-reserved body fields merged into every request.
    #[serde(default)]
    pub extra_body: BTreeMap<String, serde_json::Value>,
    /// Named adapter workarounds.
    #[serde(default)]
    pub quirks: Vec<String>,
    /// Routes a run may fall back to, in order (ADR-0025).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,
    /// When tools go out with `strict: true`: `auto` (default), `on` or `off`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<String>,
    /// Which schema keywords tools keep: `full` (default) or `basic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_keywords: Option<String>,
    /// Where the merged instructions go: Chat `system` (default) or `developer`; Responses `top_level` (default) or
    /// `developer`; Messages `top_level` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_role: Option<String>,
    /// How long to wait for the model's first byte, 5 to 900 seconds; absent, 300 for a local route and 60 otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_response_secs: Option<u32>,
}

const fn enabled() -> bool {
    true
}

impl RouteConfig {
    /// The record as releases before `stream_usage` became optional wrote it: the field always present, `false` when
    /// the file left it out. The seed's digest of a route was taken over that form, so the seeder compares against it
    /// once to tell an unchanged file from a changed one (a one-time upgrade; the next release can drop it).
    #[must_use]
    pub fn legacy_form(&self) -> Self {
        Self {
            stream_usage: Some(self.stream_usage.unwrap_or(false)),
            ..self.clone()
        }
    }
}

impl From<RouteConfig> for Route {
    fn from(value: RouteConfig) -> Self {
        Self {
            name: value.name,
            protocol: value.protocol,
            base_url: value.base_url,
            model: value.model,
            auth: value.auth,
            key_ref: value.key_ref,
            limits: Limits {
                context_window: value.context_window,
                max_output: value.max_output,
                concurrency: value.concurrency,
                requests_per_minute: value.requests_per_minute,
            },
            native_tools: value.native_tools,
            parallel_tools: value.parallel_tools,
            vision: value.vision,
            reasoning: value.reasoning,
            cache_hint: value.cache_hint,
            stream_usage: value.stream_usage,
            extra_headers: value.extra_headers,
            extra_body: value.extra_body,
            quirks: value.quirks,
            fallbacks: value.fallbacks,
            strict: value.strict,
            schema_keywords: value.schema_keywords,
            instructions_role: value.instructions_role,
            first_response_secs: value.first_response_secs,
        }
    }
}

impl From<Route> for RouteConfig {
    fn from(value: Route) -> Self {
        Self {
            name: value.name,
            display_name: None,
            protocol: value.protocol,
            base_url: value.base_url,
            model: value.model,
            auth: value.auth,
            key_ref: value.key_ref,
            context_window: value.limits.context_window,
            max_output: value.limits.max_output,
            concurrency: value.limits.concurrency,
            requests_per_minute: value.limits.requests_per_minute,
            native_tools: value.native_tools,
            parallel_tools: value.parallel_tools,
            vision: value.vision,
            reasoning: value.reasoning,
            cache_hint: value.cache_hint,
            stream_usage: value.stream_usage,
            extra_headers: value.extra_headers,
            extra_body: value.extra_body,
            quirks: value.quirks,
            fallbacks: value.fallbacks,
            strict: value.strict,
            schema_keywords: value.schema_keywords,
            instructions_role: value.instructions_role,
            first_response_secs: value.first_response_secs,
        }
    }
}
