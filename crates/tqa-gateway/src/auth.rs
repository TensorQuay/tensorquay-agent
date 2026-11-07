//! Cookie sessions, one-time bootstrap and ownership-aware user identities.

mod accounts;
pub(crate) mod api_tokens;
mod endpoints;
pub(crate) mod invitations;
mod sessions;
pub(crate) mod users;
pub(crate) use endpoints::{bootstrap, change_password, login, logout, me, setup};
pub(crate) use sessions::{Authz, SessionAuthz, TOKEN_PREFIX};

mod password;
mod policy;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod primitive_tests;
mod rate;
pub(crate) mod token;

pub(crate) use password::Passwords;
pub(crate) use policy::check_origin;
pub(crate) use rate::{LoginRate, TokenFailures};

use crate::GatewayError;
use serde::{Deserialize, Serialize};

/// Account role; administrators may list metadata but ownership still protects events and actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Installation administrator.
    Admin,
    /// Ordinary conversation owner.
    Member,
}

impl Role {
    pub(crate) fn parse(value: &str) -> Result<Self, GatewayError> {
        match value {
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            _ => Err(GatewayError::StoreUnavailable),
        }
    }
}

/// Public account metadata; credentials and password hashes are never part of this response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    /// Application-generated UUID v7 account identifier.
    pub id: uuid::Uuid,
    /// Normalized sign-in address.
    pub email: String,
    /// The account's authorization role.
    pub role: Role,
}

/// A record id from the path, parsed at the boundary into a typed error instead of the router's plain-text one.
///
/// # Errors
/// `400 invalid_request` for anything but a UUID.
pub(crate) fn path_id(value: &str) -> Result<uuid::Uuid, GatewayError> {
    uuid::Uuid::parse_str(value)
        .map_err(|_| GatewayError::BadRequest("the id is not a valid identifier"))
}
