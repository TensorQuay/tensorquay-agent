//! Agent-team request rules (ADR-0034, the slice S1 contract): `start_member`'s brief and the posts, each refusal
//! naming the field and the reason. Pure: a reference is checked through the caller's lookup (GW-U-125).
use crate::error::{GatewayError, InvalidReason};
use tqa_types::{
    MAX_BRIEF_BYTES, MAX_BRIEF_PART_BYTES, MAX_ROLE_BYTES, StartMember, TeamMessageType, TeamPost,
    valid_profile_name,
};

/// The most files one report hands back.
pub(crate) const MAX_REPORT_FILES: usize = 32;
/// The longest path a report names, in bytes.
const MAX_PATH_BYTES: usize = 1_024;
/// The longest member name: a role plus its `-N` suffix.
const MAX_MEMBER_NAME_BYTES: usize = MAX_ROLE_BYTES + 8;

const fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

/// A text field: no NUL, not empty, at most `max` bytes.
fn text(field: &'static str, value: &str, max: usize) -> Result<(), GatewayError> {
    if value.contains('\0') {
        return Err(invalid(field, InvalidReason::ValueNul));
    }
    if value.is_empty() {
        return Err(invalid(field, InvalidReason::ValueEmpty));
    }
    if value.len() > max {
        return Err(invalid(field, InvalidReason::ValueTooLong));
    }
    Ok(())
}

/// `done`'s summary: no NUL, not empty, at most the `done` message's 8 KiB.
///
/// # Errors
/// `invalid_request` naming `summary`.
pub(crate) fn validate_done(summary: &str) -> Result<(), GatewayError> {
    text("summary", summary, TeamMessageType::Done.max_text_bytes())
}

/// Checks `start_member`'s request: the role is one short line of letters, digits, spaces, `-` and `_`; the brief's
/// objective is required, every part at most [`MAX_BRIEF_PART_BYTES`] and the whole at most [`MAX_BRIEF_BYTES`];
/// a profile is a well-formed ID. Whether the person may use that profile is the store's check.
///
/// # Errors
/// `invalid_request` naming the first field that breaks its rule.
pub(crate) fn validate_start(request: &StartMember) -> Result<(), GatewayError> {
    text("role", &request.role, MAX_ROLE_BYTES)?;
    if !request
        .role
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
    {
        return Err(invalid("role", InvalidReason::NameInvalid));
    }
    let parts = request.brief.parts();
    for (field, part) in &parts {
        text(field, part, MAX_BRIEF_PART_BYTES)?;
    }
    if parts.iter().map(|(_, part)| part.len()).sum::<usize>() > MAX_BRIEF_BYTES {
        return Err(invalid("brief", InvalidReason::ValueTooLong));
    }
    if let Some(profile) = &request.profile
        && !valid_profile_name(profile)
    {
        return Err(invalid("profile", InvalidReason::ParameterInvalid));
    }
    Ok(())
}

/// Checks a post: its text against its type's limit, an `assign`'s addressee, a report's file paths, and that a
/// `question_seq` names a message of this agent team that it may answer (`refs` looks a seq up and says its type): an
/// answer a lead's `question`; a decision that too, or a member's `ask`.
///
/// # Errors
/// `invalid_request` naming the field, or `unknown_reference` for a reference to no message of the right type.
pub(crate) fn validate_post(
    post: &TeamPost,
    refs: impl Fn(u64) -> Option<TeamMessageType>,
) -> Result<(), GatewayError> {
    text("text", post.text(), post.message_type().max_text_bytes())?;
    let answers = |seq: u64, kinds: &[TeamMessageType]| {
        if refs(seq).is_some_and(|kind| kinds.contains(&kind)) {
            Ok(())
        } else {
            Err(GatewayError::UnknownReference {
                field: "question_seq",
            })
        }
    };
    match post {
        TeamPost::Assign { to, .. } => text("to", to, MAX_MEMBER_NAME_BYTES),
        // A member answers its lead's question, never another member's ask; the lead decides either.
        TeamPost::Answer { question_seq, .. } => {
            answers(*question_seq, &[TeamMessageType::Question])
        }
        TeamPost::Decision {
            question_seq: Some(question_seq),
            ..
        } => answers(
            *question_seq,
            &[TeamMessageType::Question, TeamMessageType::Ask],
        ),
        TeamPost::Report { files, .. } => report_files(files),
        TeamPost::Question { .. }
        | TeamPost::Decision { .. }
        | TeamPost::PersonQuestion { .. }
        | TeamPost::Ask { .. } => Ok(()),
    }
}

/// A report's files: at most [`MAX_REPORT_FILES`], each a relative path inside the workspace.
pub(crate) fn report_files(files: &[String]) -> Result<(), GatewayError> {
    if files.len() > MAX_REPORT_FILES {
        return Err(invalid("files", InvalidReason::ValueTooLong));
    }
    for path in files {
        text("files", path, MAX_PATH_BYTES)?;
        if path.starts_with('/') || path.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(invalid("files", InvalidReason::ParameterInvalid));
        }
    }
    Ok(())
}

/// A member's name: its role, or its role with the next free number when the agent team already has that name.
pub(crate) fn unique_name(role: &str, taken: &[&str]) -> String {
    // Compared as PostgreSQL's lower() does in the unique index (review L3), for any letters, not only ASCII.
    let free = |name: &str| {
        !taken
            .iter()
            .any(|taken| taken.to_lowercase() == name.to_lowercase())
    };
    if free(role) {
        return role.to_owned();
    }
    // The next free number: `taken` is finite, so one of the first `taken.len() + 1` numbers is free and the count
    // ends there.
    let mut number = 2_usize;
    loop {
        let name = format!("{role}-{number}");
        if free(&name) {
            return name;
        }
        number += 1;
    }
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
