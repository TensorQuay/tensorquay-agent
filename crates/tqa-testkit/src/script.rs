//! Synthetic completion scripts and deliberate transport-framing variations.

use std::time::Duration;
use tqa_llm::{FinishReason, Usage};

/// Valid SSE variations used to test incremental decoders.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MalformedSse {
    /// Emit one byte at a time, including inside multibyte scalars.
    SplitUtf8,
    /// Split each data-field prefix after its first two bytes.
    SplitData,
    /// Use carriage-return/line-feed terminators.
    CrLf,
    /// Add comment and blank keep-alive records.
    Comments,
    /// End after the finish/usage records without the done sentinel.
    MissingDone,
}

/// One synthetic output or fault in a completion script.
#[derive(Clone, Debug)]
pub enum Scripted {
    /// Emit a text delta.
    Text(String),
    /// Emit a reasoning delta.
    Reasoning(String),
    /// Split a tool call's JSON arguments into the requested number of fragments.
    ToolCall {
        /// The tool name.
        name: String,
        /// Argument JSON, which may deliberately be malformed.
        arguments_json: String,
        /// Positive fragment count, capped by the script compiler.
        split_into: usize,
    },
    /// Emit an explicitly indexed fragment, allowing interleaved parallel calls.
    ToolCallDelta {
        /// The completion's call index.
        index: u32,
        /// Optional call identifier.
        id: Option<String>,
        /// Optional function name.
        name: Option<String>,
        /// Argument text appended to that index.
        arguments_delta: String,
    },
    /// Emit usage after the terminal finish record.
    Usage(Usage),
    /// Return this HTTP status and body instead of a stream.
    Status {
        /// The response status code.
        code: u16,
        /// A synthetic response body.
        body: String,
        /// Optional Retry-After header, in seconds or HTTP-date form.
        retry_after: Option<String>,
    },
    /// Set this response header (name, value) on the response, a `Status` one or a stream (over its content type).
    Header(String, String),
    /// End after this many logical output events, before automatic terminal records.
    CutAfter(usize),
    /// Apply one framing variation to the stream.
    MalformedSse(MalformedSse),
    /// Delay the next event using the server's injected sleeper.
    Delay(Duration),
    /// Override the automatic terminal finish reason.
    Finish(FinishReason),
    /// Supply raw data-field JSON for malformed-response tests.
    RawJson(String),
}

/// One HTTP request's scripted response; scripts are consumed in FIFO order.
#[derive(Clone, Debug)]
pub struct Script {
    /// Delay before sending headers, using the injected sleeper.
    pub header_delay: Duration,
    /// Ordered output records and fault controls.
    pub events: Vec<Scripted>,
}

impl Script {
    /// Creates a script with no header delay and the exact supplied events.
    #[must_use]
    pub fn new(events: Vec<Scripted>) -> Self {
        Self {
            header_delay: Duration::ZERO,
            events,
        }
    }
}
