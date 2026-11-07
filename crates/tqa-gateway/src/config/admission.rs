//! `[admission]`: how many sandboxes run at once and per user (ARCHITECTURE §4.4, ADR-0016).
use crate::GatewayError;
use serde::Deserialize;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionInput {
    slots: Option<u32>,
    per_user: Option<u32>,
}

/// The slot budget and the per-user cap. Host-derived slots (a share of memory and cores) are not automatic yet.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Admission {
    pub slots: u32,
    pub per_user: u32,
}

/// Defaults: `slots` = `sandbox.max_live`, `per_user` = `slots`, so a one-person install is not capped below the host.
///
/// # Errors
/// Rejects zero values and budgets above `sandbox.max_live`.
pub(super) fn admission(input: AdmissionInput, max_live: u32) -> Result<Admission, GatewayError> {
    let slots = input.slots.unwrap_or(max_live);
    let per_user = input.per_user.unwrap_or(slots);
    if slots == 0 || slots > max_live || per_user == 0 || per_user > slots {
        return Err(GatewayError::Configuration(
            "admission slots must be 1 through sandbox.max_live, and per_user 1 through slots",
        ));
    }
    Ok(Admission { slots, per_user })
}
