//! Why a route is refused: one typed reason per rule, with a stable code for APIs and the message the file
//! loader has always printed.

/// A route that fails validation. [`RouteInvalid::code`] is stable for API clients; the message names the rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum RouteInvalid {
    /// The route name is empty.
    #[error("name and model must not be empty")]
    RouteNameInvalid,
    /// The route name is `new`, the address of the web's Add route page (GW-U-098).
    #[error(
        "the route name new is reserved: its page's address is taken by Add route; choose another name"
    )]
    RouteNameReserved,
    /// The model identifier is empty.
    #[error("name and model must not be empty")]
    ModelEmpty,
    /// The base URL does not parse, or is not plain HTTP(S) without credentials, query or fragment.
    #[error("{0}")]
    BaseUrlInvalid(&'static str),
    /// An authenticated route names no secret-store reference.
    #[error("authenticated route requires key_ref")]
    KeyRefRequired,
    /// The key reference is not `secrets/<name>`: it may be a pasted key, which must never be stored or echoed.
    #[error("key_ref must be a secret reference, secrets/<name>")]
    KeyRefNotReference,
    /// The key reference names a user's own credential, which only its owner's calls may use (FR-19, FR-24).
    #[error("key_ref must not name a user's own credential")]
    KeyRefReserved,
    /// The credential header name is not a valid HTTP header name.
    #[error("invalid auth header name")]
    AuthHeaderInvalid,
    /// The credential query parameter name is empty or not alphanumeric.
    #[error("invalid auth query name")]
    AuthQueryInvalid,
    /// The reasoning field is empty or owned, or its default is not an allowed value.
    #[error("reasoning default or field is invalid")]
    ReasoningInvalid,
    /// A thinking budget table is where it does not apply, missing, or does not match the allowed values.
    #[error("{0}")]
    BudgetInvalid(&'static str),
    /// A protocol that requires an output limit has none.
    #[error("{0}")]
    MaxOutputRequired(&'static str),
    /// A declared limit is zero, or the output limit exceeds the context window.
    #[error("limits must be positive and max_output must fit context_window")]
    LimitOutOfRange,
    /// An `extra_body` field would override a field the harness sets.
    #[error("extra_body collides with a harness-owned field")]
    ExtraBodyReserved,
    /// An extra header's name or value is not valid HTTP.
    #[error("{0}")]
    ExtraHeaderInvalid(&'static str),
    /// An extra header would set a header the protocol pins.
    #[error("extra_headers must not set the protocol version header")]
    ExtraHeaderReserved,
    /// An extra header would carry a credential.
    #[error("extra_headers must not contain credentials")]
    ExtraHeaderCredential,
    /// A named adapter workaround is not implemented.
    #[error("unsupported adapter quirk")]
    QuirkUnsupported,
    /// `strict` is not `auto`, `on` or `off`.
    #[error("strict must be auto, on or off")]
    StrictInvalid,
    /// `schema_keywords` is not `full` or `basic`.
    #[error("schema_keywords must be full or basic")]
    SchemaKeywordsInvalid,
    /// `instructions_role` is not a value the route's protocol accepts; names the accepted ones.
    #[error("instructions_role must be {0}")]
    InstructionsRoleInvalid(&'static str),
    /// `first_response_secs` is outside 5 to 900 seconds.
    #[error("first_response_secs must be 5 to 900 seconds")]
    FirstResponseInvalid,
    /// A fallback list is too long, names the route itself, an unknown or repeated route, or forms a cycle.
    #[error("{0}")]
    FallbackInvalid(&'static str),
}

impl RouteInvalid {
    /// The stable code an API reports for this reason.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::RouteNameInvalid => "route_name_invalid",
            Self::RouteNameReserved => "name_reserved",
            Self::ModelEmpty => "model_empty",
            Self::BaseUrlInvalid(_) => "base_url_invalid",
            Self::KeyRefRequired => "key_ref_required",
            Self::KeyRefNotReference => "key_ref_not_reference",
            Self::KeyRefReserved => "key_ref_reserved",
            Self::AuthHeaderInvalid => "auth_header_invalid",
            Self::AuthQueryInvalid => "auth_query_invalid",
            Self::ReasoningInvalid => "reasoning_invalid",
            Self::BudgetInvalid(_) => "budget_invalid",
            Self::MaxOutputRequired(_) => "max_output_required",
            Self::LimitOutOfRange => "limit_out_of_range",
            Self::ExtraBodyReserved => "extra_body_reserved",
            Self::ExtraHeaderInvalid(_) => "extra_header_invalid",
            Self::ExtraHeaderReserved => "extra_header_reserved",
            Self::ExtraHeaderCredential => "extra_header_credential",
            Self::QuirkUnsupported => "quirk_unsupported",
            Self::StrictInvalid => "strict_invalid",
            Self::SchemaKeywordsInvalid => "schema_keywords_invalid",
            Self::InstructionsRoleInvalid(_) => "instructions_role_invalid",
            Self::FirstResponseInvalid => "first_response_invalid",
            Self::FallbackInvalid(_) => "fallback_invalid",
        }
    }
}
