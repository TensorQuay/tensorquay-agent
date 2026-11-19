//! Why a request field was refused (`details[].reason`), and the message that names it.

/// The closed list of field-refusal reasons (090 contract amendment 6). Clients map each to their own text, so a
/// code is added here, in `openapi.json` and in the clients together, and never renamed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidReason {
    /// A query or body parameter is unknown or repeated.
    ParameterInvalid,
    /// A date is not a `YYYY-MM-DD` calendar date.
    DateInvalid,
    /// `from` is after `to`.
    RangeReversed,
    /// The range spans more days than allowed.
    RangeTooLong,
    /// A date is later than tomorrow (UTC).
    DateFuture,
    /// The grouping is not one of the documented values.
    GroupInvalid,
    /// A secret name is empty, too long or has characters outside `[A-Za-z0-9_-]`.
    NameInvalid,
    /// A secret name uses a prefix reserved for per-user credentials.
    ReservedPrefix,
    /// The setting needs a desktop, and the task's environment has none (`keep_sign_in`, ADR-0039 section 6).
    NoDesktop,
    /// A secret value is empty.
    ValueEmpty,
    /// A secret value is longer than the store allows.
    ValueTooLong,
    /// A secret value contains a NUL character.
    ValueNul,
    /// A version is missing where one is required, or not a positive integer.
    VersionInvalid,
    /// The body is not the documented JSON object.
    BodyInvalid,
    /// A list cursor is not one this gateway issued.
    CursorInvalid,
    /// An egress host is not an exact name or address.
    HostInvalid,
    /// An egress wildcard is not `*.` followed by a domain.
    WildcardInvalid,
    /// `internal` set on an egress rule that isn't one exact host name (ADR-0035 amendment).
    InternalNameOnly,
    /// Egress ports are missing, zero, repeated or more than 16.
    PortsInvalid,
    /// A route name is empty, longer than 128 bytes, or has control characters.
    RouteNameInvalid,
    /// A route's model is empty.
    ModelEmpty,
    /// A route's base URL is not plain HTTP(S) without credentials, query or fragment.
    BaseUrlInvalid,
    /// An authenticated route names no secret reference.
    KeyRefRequired,
    /// A route's key reference is not `secrets/<name>` (SEC-033); the value is never echoed.
    KeyRefNotReference,
    /// A route's key reference names a user's own credential (FR-19, FR-24).
    KeyRefReserved,
    /// A route's credential header name is not a valid HTTP header name.
    AuthHeaderInvalid,
    /// A route's credential query parameter name is invalid.
    AuthQueryInvalid,
    /// A route's reasoning field or default is invalid.
    ReasoningInvalid,
    /// A route's thinking budgets do not match its reasoning values.
    BudgetInvalid,
    /// The route's protocol requires `max_output`.
    MaxOutputRequired,
    /// A route limit is zero, or `max_output` exceeds `context_window`.
    LimitOutOfRange,
    /// An `extra_body` field would override a field the harness sets.
    ExtraBodyReserved,
    /// An extra header's name or value is not valid HTTP.
    ExtraHeaderInvalid,
    /// An extra header would set a header the protocol pins.
    ExtraHeaderReserved,
    /// An extra header would carry a credential.
    ExtraHeaderCredential,
    /// A named adapter quirk is not implemented.
    QuirkUnsupported,
    /// A route's `strict` is not `auto`, `on` or `off`.
    StrictInvalid,
    /// A route's `schema_keywords` is not `full` or `basic`.
    SchemaKeywordsInvalid,
    /// A route's `instructions_role` is not a value its protocol accepts.
    InstructionsRoleInvalid,
    /// A route's `first_response_secs` is outside 5 to 900 seconds.
    FirstResponseInvalid,
    /// A fallback list is too long, names the route itself or an unknown or repeated route, or forms a cycle.
    FallbackInvalid,
    /// A route is refused for a reason this gateway does not name yet.
    RouteInvalid,
    /// A display name is empty, longer than 80 characters, or has control characters or surrounding spaces.
    DisplayNameInvalid,
    /// A limit name is not one of the overridable limits.
    LimitUnknown,
    /// `admission.per_user` would exceed `admission.slots`.
    LimitOrder,
    /// A connector's auth does not fit together: a header without the header scheme, or a credential with no scheme.
    AuthInvalid,
    /// A secret reference is not a valid secret name.
    SecretRefInvalid,
    /// A connector's shared credential names a user's own credential (FR-19, FR-24).
    SecretRefReserved,
    /// A rotation reminder is not 1 through 3650 days, or `null`.
    RotateAfterInvalid,
    /// A secret's consumer list is not route or connector entries with valid names, or has too many.
    ConsumersInvalid,
    /// A tool filter is not exactly one bounded list of distinct names.
    ToolFilterInvalid,
    /// The field may only be set in the configuration file, never through the API.
    FileOnly,
    /// A template image is a tag or carries credentials instead of a digest pin (ADR-0032).
    ImageNotPinned,
    /// Template capabilities repeat or are more than five.
    CapabilitiesInvalid,
    /// A list of names repeats a name, is longer than allowed, or holds a name that is not well formed (ADR-0012).
    ListInvalid,
    /// A name refers to a route, template, tool, server or connector that does not exist (ADR-0012).
    UnknownReference,
    /// The conversation's profile sets the field; a request may not change or widen it (ADR-0012).
    SetByProfile,
    /// Only a shared profile may set the field.
    SharedOnly,
    /// A channel's decision rule is not `human` or `majority` (ADR-0014).
    RuleInvalid,
    /// A channel message's field is out of bounds or not allowed; the field is its path, e.g. `body.title` (ADR-0014).
    MessageInvalid,
    /// The current password given to change it is not the account's password.
    PasswordIncorrect,
    /// A new password breaks the first administrator's rules: at least 12 characters, not the email, not common.
    PasswordWeak,
    /// The new password is the current one.
    PasswordUnchanged,
    /// A description is empty, longer than 160 characters, not one line, or has surrounding spaces (GW-I-132).
    DescriptionInvalid,
    /// The name is reserved: `new` is the address of the Add page, so no environment may take it (GW-U-093); or a host
    /// key's name is one the task's environment relies on (ADR-0037 Amendment 1, GW-U-103).
    NameReserved,
    /// A host key's address is not an `https://` address (GW-U-102).
    AddressNotHttps,
    /// A host key's address has credentials, a query, a fragment, or a path over 512 bytes (GW-U-102).
    AddressNotPlain,
    /// A `basic` host key needs its user name here (ADR-0037 §5).
    UserNameRequired,
    /// A `header` host key needs the header's name (ADR-0037 §5).
    HeaderNameRequired,
    /// This way of sending a key is not available yet (the `header` scheme, ADR-0037's second slice).
    SchemeNotSupported,
    /// A value holds a character no header can carry (a line break).
    ValueInvalid,
    /// A host key's name is the caller's own or a usable shared key's (the `409 name_taken` detail).
    NameTaken,
    /// A shared key's name is a member's own key's, whom it never names (the `409 name_taken_by_user` detail).
    NameTakenByUser,
    /// The field belonged to a feature this release removed (ADR-0037 §7: `git_url`, `repositories`).
    Removed,
    /// A secret's name, or a reference's, looks like a key rather than a name (SEC-033, `tqa_types::is_key_shaped_name`):
    /// most likely a key pasted where its name belongs. The value is never echoed.
    NameKeyShaped,
}

impl InvalidReason {
    /// Every reason, in documentation order.
    pub const ALL: [Self; 75] = [
        Self::ParameterInvalid,
        Self::DateInvalid,
        Self::RangeReversed,
        Self::RangeTooLong,
        Self::DateFuture,
        Self::GroupInvalid,
        Self::NameInvalid,
        Self::ReservedPrefix,
        Self::NoDesktop,
        Self::ValueEmpty,
        Self::ValueTooLong,
        Self::ValueNul,
        Self::VersionInvalid,
        Self::BodyInvalid,
        Self::CursorInvalid,
        Self::HostInvalid,
        Self::WildcardInvalid,
        Self::InternalNameOnly,
        Self::PortsInvalid,
        Self::RouteNameInvalid,
        Self::ModelEmpty,
        Self::BaseUrlInvalid,
        Self::KeyRefRequired,
        Self::KeyRefNotReference,
        Self::KeyRefReserved,
        Self::AuthHeaderInvalid,
        Self::AuthQueryInvalid,
        Self::ReasoningInvalid,
        Self::BudgetInvalid,
        Self::MaxOutputRequired,
        Self::LimitOutOfRange,
        Self::ExtraBodyReserved,
        Self::ExtraHeaderInvalid,
        Self::ExtraHeaderReserved,
        Self::ExtraHeaderCredential,
        Self::QuirkUnsupported,
        Self::StrictInvalid,
        Self::SchemaKeywordsInvalid,
        Self::InstructionsRoleInvalid,
        Self::FirstResponseInvalid,
        Self::FallbackInvalid,
        Self::RouteInvalid,
        Self::DisplayNameInvalid,
        Self::LimitUnknown,
        Self::LimitOrder,
        Self::AuthInvalid,
        Self::SecretRefInvalid,
        Self::SecretRefReserved,
        Self::RotateAfterInvalid,
        Self::ConsumersInvalid,
        Self::ToolFilterInvalid,
        Self::FileOnly,
        Self::ImageNotPinned,
        Self::CapabilitiesInvalid,
        Self::ListInvalid,
        Self::UnknownReference,
        Self::SetByProfile,
        Self::SharedOnly,
        Self::RuleInvalid,
        Self::MessageInvalid,
        Self::PasswordIncorrect,
        Self::PasswordWeak,
        Self::PasswordUnchanged,
        Self::DescriptionInvalid,
        Self::NameReserved,
        Self::AddressNotHttps,
        Self::AddressNotPlain,
        Self::UserNameRequired,
        Self::HeaderNameRequired,
        Self::SchemeNotSupported,
        Self::ValueInvalid,
        Self::NameTaken,
        Self::NameTakenByUser,
        Self::Removed,
        Self::NameKeyShaped,
    ];

    /// The wire code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParameterInvalid => "parameter_invalid",
            Self::DateInvalid => "date_invalid",
            Self::RangeReversed => "range_reversed",
            Self::RangeTooLong => "range_too_long",
            Self::DateFuture => "date_future",
            Self::GroupInvalid => "group_invalid",
            Self::NameInvalid => "name_invalid",
            Self::ReservedPrefix => "reserved_prefix",
            Self::NoDesktop => "no_desktop",
            Self::ValueEmpty => "value_empty",
            Self::ValueTooLong => "value_too_long",
            Self::ValueNul => "value_nul",
            Self::VersionInvalid => "version_invalid",
            Self::BodyInvalid => "body_invalid",
            Self::CursorInvalid => "cursor_invalid",
            Self::HostInvalid => "host_invalid",
            Self::WildcardInvalid => "wildcard_invalid",
            Self::InternalNameOnly => "internal_name_only",
            Self::PortsInvalid => "ports_invalid",
            Self::RouteNameInvalid => "route_name_invalid",
            Self::ModelEmpty => "model_empty",
            Self::BaseUrlInvalid => "base_url_invalid",
            Self::KeyRefRequired => "key_ref_required",
            Self::KeyRefNotReference => "key_ref_not_reference",
            Self::KeyRefReserved => "key_ref_reserved",
            Self::AuthHeaderInvalid => "auth_header_invalid",
            Self::AuthQueryInvalid => "auth_query_invalid",
            Self::ReasoningInvalid => "reasoning_invalid",
            Self::BudgetInvalid => "budget_invalid",
            Self::MaxOutputRequired => "max_output_required",
            Self::LimitOutOfRange => "limit_out_of_range",
            Self::ExtraBodyReserved => "extra_body_reserved",
            Self::ExtraHeaderInvalid => "extra_header_invalid",
            Self::ExtraHeaderReserved => "extra_header_reserved",
            Self::ExtraHeaderCredential => "extra_header_credential",
            Self::QuirkUnsupported => "quirk_unsupported",
            Self::StrictInvalid => "strict_invalid",
            Self::SchemaKeywordsInvalid => "schema_keywords_invalid",
            Self::InstructionsRoleInvalid => "instructions_role_invalid",
            Self::FirstResponseInvalid => "first_response_invalid",
            Self::FallbackInvalid => "fallback_invalid",
            Self::RouteInvalid => "route_invalid",
            Self::DisplayNameInvalid => "display_name_invalid",
            Self::LimitUnknown => "limit_unknown",
            Self::LimitOrder => "limit_order",
            Self::AuthInvalid => "auth_invalid",
            Self::SecretRefInvalid => "secret_ref_invalid",
            Self::SecretRefReserved => "secret_ref_reserved",
            Self::RotateAfterInvalid => "rotate_after_invalid",
            Self::ConsumersInvalid => "consumers_invalid",
            Self::ToolFilterInvalid => "tool_filter_invalid",
            Self::FileOnly => "file_only",
            Self::ImageNotPinned => "image_not_pinned",
            Self::CapabilitiesInvalid => "capabilities_invalid",
            Self::ListInvalid => "list_invalid",
            Self::UnknownReference => "unknown_reference",
            Self::SetByProfile => "set_by_profile",
            Self::SharedOnly => "shared_only",
            Self::RuleInvalid => "rule_invalid",
            Self::MessageInvalid => "message_invalid",
            Self::PasswordIncorrect => "password_incorrect",
            Self::PasswordWeak => "password_weak",
            Self::PasswordUnchanged => "password_unchanged",
            Self::DescriptionInvalid => "description_invalid",
            Self::NameReserved => "name_reserved",
            Self::AddressNotHttps => "address_not_https",
            Self::AddressNotPlain => "address_not_plain",
            Self::UserNameRequired => "user_name_required",
            Self::HeaderNameRequired => "header_name_required",
            Self::SchemeNotSupported => "scheme_not_supported",
            Self::ValueInvalid => "value_invalid",
            Self::NameTaken => "name_taken",
            Self::NameTakenByUser => "name_taken_by_user",
            Self::Removed => "removed",
            Self::NameKeyShaped => "name_key_shaped",
        }
    }
}

/// [`GatewayError::Invalid`]'s message: the field and its reason, or for a removed Git field (ADR-0037 §7) what to do.
pub(super) fn invalid_message(field: &str, reason: InvalidReason) -> String {
    match reason {
        InvalidReason::Removed => format!(
            "invalid request: {field} was removed in this release: clone with plain git and a key under Keys"
        ),
        _ => format!("invalid request: {field} ({reason})"),
    }
}

impl std::fmt::Display for InvalidReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
