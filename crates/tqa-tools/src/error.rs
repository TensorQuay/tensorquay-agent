//! Stable failures without argument or file content in diagnostics.

/// A tool failure suitable for a bounded protocol error.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ToolError {
    /// Fixed Git relay or snapshot failure, without upstream output or credentials.
    #[error(transparent)]
    GitPush(#[from] tqa_types::GitPushFailure),
    /// A permission or protected-path invariant rejected the action.
    #[error("tool action denied")]
    Denied(String),
    /// The gateway proxy refused the validated destination.
    #[error("network access denied by egress policy")]
    EgressDenied {
        /// The destination only; paths, queries and credentials are never included.
        target: tqa_types::ErrorTarget,
    },
    /// The proxy rejected this runtime's credential or source binding.
    #[error("runtime proxy authentication failed")]
    ProxyAuthentication,
    /// Arguments failed validation at the supplied JSON pointer.
    #[error("invalid tool arguments")]
    InvalidArgs {
        /// JSON pointer to the failing value.
        path: String,
        /// Content-free description of the failed constraint.
        detail: String,
    },
    /// Resolution escaped the canonical workspace root.
    #[error("path is outside the workspace")]
    PathOutsideWorkspace,
    /// The path is malformed or cannot be traversed.
    #[error("invalid workspace path")]
    PathInvalid(String),
    /// The requested file does not exist.
    #[error("file not found")]
    NotFound,
    /// The workspace's filesystem has no space left (ENOSPC or EDQUOT).
    #[error("the workspace is full")]
    WorkspaceFull,
    /// A filesystem or process operation failed.
    #[error("tool I/O failed")]
    Io(String),
    /// The configured deadline elapsed.
    #[error("tool deadline elapsed")]
    Timeout,
    /// The caller cancelled the operation.
    #[error("tool cancelled")]
    Cancelled,
    /// A bounded input or output cannot fit.
    #[error("tool byte limit exceeded ({max})")]
    OutputTooLarge {
        /// Maximum permitted bytes.
        max: usize,
    },
    /// An MCP server, connector or the gateway's MCP proxy refused or failed a call, with its validated stable code
    /// (ADR-0031). Build it with [`ToolError::mcp`].
    #[error("MCP call failed")]
    Mcp {
        /// A stable code matching `^[a-z_]{1,64}$`, or `mcp_error`.
        code: String,
        /// A content-free sentence for the model.
        detail: String,
    },
    /// A channel route refused or failed a call, with the gateway's stable code (ADR-0014). Build it with
    /// [`ToolError::channel`].
    #[error("channel call failed")]
    Channel {
        /// A stable code matching `^[a-z_]{1,64}$`, or `channel_error`.
        code: String,
        /// A content-free sentence for the model.
        detail: String,
    },
    /// The `computer` tool refused or failed an action, with its stable code (`stale_ref`, `display_unavailable`,
    /// `action_failed`, ...; ADR-0039's runtime contract). Build it with [`ToolError::screen`].
    #[error("computer action failed")]
    Screen {
        /// A stable code matching `^[a-z_]{1,64}$`, or `screen_error`.
        code: String,
        /// A sentence for the model, holding no screen text.
        detail: String,
    },
    /// The requested tool or operation is unavailable.
    #[error("unsupported tool operation")]
    Unsupported(String),
}

impl ToolError {
    /// Returns the actionable tool failure for the model, at most 500 characters.
    /// Callers put content-free constraint details in errors, never file contents.
    /// The agent loop renders failures with this method; logs use `Display`.
    #[must_use]
    pub fn model_message(&self) -> String {
        const MAX_ERROR_CHARS: usize = 500;
        let reason = match self {
            Self::Denied(reason)
            | Self::PathInvalid(reason)
            | Self::Io(reason)
            | Self::Unsupported(reason) => reason.clone(),
            // The stable code leads the message: the web maps it (WEB-U-045).
            Self::Mcp { code, detail } | Self::Channel { code, detail } | Self::Screen { code, detail } => {
                return bound(&format!("{code}: {detail}"), MAX_ERROR_CHARS);
            }
            Self::InvalidArgs { path, detail } => format!("{path}: {detail}"),
            // C1: plain words and who can change it; the code is for the logs and tool.end's failure (TOOL-U-040).
            Self::EgressDenied { target } => {
                let site = match target.port() {
                    80 | 443 => target.host().to_owned(),
                    port => format!("{} (port {port})", target.host()),
                };
                // Without the code prefix: the model passes on what it reads (C1).
                return bound(
                    &format!(
                        "{site} isn't on this task's allowed sites. An administrator can allow it in Settings > \
                         Internet access."
                    ),
                    MAX_ERROR_CHARS,
                );
            }
            Self::WorkspaceFull => {
                "no space is left in the workspace; free space (delete files you no longer need) and retry".into()
            }
            _ => self.to_string(),
        };
        bound(&format!("{}: {reason}", self.code()), MAX_ERROR_CHARS)
    }
    /// An MCP failure whose code is kept when it matches `^[a-z_]{1,64}$`; anything else, from an untrusted remote, is
    /// `mcp_error`.
    #[must_use]
    pub fn mcp(code: &str, detail: &str) -> Self {
        let valid = (1..=64).contains(&code.len())
            && code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_');
        Self::Mcp {
            code: if valid { code } else { "mcp_error" }.to_owned(),
            detail: detail.to_owned(),
        }
    }
    /// A channel failure whose code is kept when it matches `^[a-z_]{1,64}$`; anything else is `channel_error`.
    #[must_use]
    pub fn channel(code: &str, detail: &str) -> Self {
        let valid = (1..=64).contains(&code.len())
            && code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_');
        Self::Channel {
            code: if valid { code } else { "channel_error" }.to_owned(),
            detail: detail.to_owned(),
        }
    }
    /// A `computer` failure whose code is kept when it matches `^[a-z_]{1,64}$`; anything else is `screen_error`.
    #[must_use]
    pub fn screen(code: &str, detail: &str) -> Self {
        let valid = (1..=64).contains(&code.len())
            && code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_');
        Self::Screen {
            code: if valid { code } else { "screen_error" }.to_owned(),
            detail: detail.to_owned(),
        }
    }
    /// The destination a refusal names, for clients' own wording (C1); `None` for every other failure.
    #[must_use]
    pub const fn target(&self) -> Option<&tqa_types::ErrorTarget> {
        match self {
            Self::EgressDenied { target } => Some(target),
            _ => None,
        }
    }
    /// Returns a stable machine-readable failure code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::GitPush(failure) => failure.code(),
            Self::Denied(_) => "denied",
            Self::EgressDenied { .. } => "egress_denied",
            Self::ProxyAuthentication => "proxy_authentication",
            Self::InvalidArgs { .. } => "invalid_args",
            Self::PathOutsideWorkspace => "path_outside_workspace",
            Self::PathInvalid(_) => "path_invalid",
            Self::NotFound => "not_found",
            Self::WorkspaceFull => "workspace_full",
            Self::Io(_) => "io",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::OutputTooLarge { .. } => "output_too_large",
            Self::Unsupported(_) => "unsupported",
            Self::Mcp { .. } => "mcp_error",
            Self::Channel { .. } => "channel_error",
            Self::Screen { .. } => "screen_error",
        }
    }
    /// The code a failed call's `tool.end` carries: a screen refusal's own code (`stale_ref`, `screenshot_limit`, ...),
    /// which a client words the step from, and [`Self::code`] for every other failure.
    #[must_use]
    pub fn failure_code(&self) -> &str {
        match self {
            Self::Screen { code, .. } => code,
            other => other.code(),
        }
    }
    pub(crate) fn invalid(path: &str, detail: &str) -> Self {
        Self::InvalidArgs {
            path: path.into(),
            detail: detail.into(),
        }
    }
}

impl From<std::io::Error> for ToolError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => {
                Self::WorkspaceFull
            }
            kind => Self::Io(format!("{kind:?}")),
        }
    }
}

/// At most `max` characters, ending in an ellipsis when cut.
fn bound(message: &str, max: usize) -> String {
    if let Some((boundary, _)) = message.char_indices().nth(max) {
        let mut bounded = message[..boundary].to_owned();
        bounded.pop();
        bounded.push('…');
        bounded
    } else {
        message.to_owned()
    }
}
