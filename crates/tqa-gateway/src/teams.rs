//! Agent teams (ADR-0034, v1.1), behind the install switch `[agent_teams] enabled`: the rules, the storage and the
//! runtime routes, built slice by slice (DEV-PLAN Phase 10).
// The cursor's rules as a pure model, tested (GW-U-126) and mirrored by the store's SQL.
mod acceptance;
pub(crate) mod budget;
pub(crate) mod cap;
pub(crate) mod changed;
#[cfg(test)]
mod cursor;
pub(crate) mod files;
mod files_rules;
pub(crate) mod first_turn;
pub(crate) mod hand_back;
pub(crate) mod member_state;
pub(crate) mod members_api;
pub(crate) mod pending;
pub(crate) mod people;
mod people_read;
mod people_rules;
pub(crate) mod people_write;
pub(crate) mod place;
mod place_shape;
mod read_shape;
pub(crate) mod reconciler;
mod roles;
pub(crate) mod runtime;
pub(crate) mod start;
pub(crate) mod stop;
pub(crate) mod store;
mod validate;
pub(crate) mod wake;
