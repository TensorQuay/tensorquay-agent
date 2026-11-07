//! The workspaces list's paging, sort and filter (GW-I-117). A page ends at an opaque cursor holding its sort, its
//! filter and the last row's sort keys exactly as the database computed them, so the next page starts strictly after
//! it and rows added meanwhile never repeat or shift one: `recent` (last used, else created) and `created` newest
//! first, `name` A to Z (`lower(name)`), each with the id as the tie-break. A search (`q`, GW-I-122) is part of the
//! list too: the cursor holds a short digest of it, never the text, so a cursor stays within its length bound.
use crate::{GatewayError, InvalidReason};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use uuid::Uuid;

/// The most a page may hold.
pub(super) const MAX_PAGE: u32 = 100;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sort {
    /// Last used (a lease or a run), else created; newest first.
    Recent,
    /// Created, newest first: the list's order before paging existed.
    #[default]
    Created,
    /// By name, A to Z.
    Name,
}

impl Sort {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Created => "created",
            Self::Name => "name",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Filter {
    #[default]
    All,
    /// Its write lease is held (a task's sandbox is using it).
    InUse,
    /// Not in use.
    Idle,
    /// Someone else's, shared with the caller.
    SharedWithMe,
}

impl Filter {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::InUse => "in_use",
            Self::Idle => "idle",
            Self::SharedWithMe => "shared_with_me",
        }
    }
}

/// The cursor's key for search text `q`: none without a search, else the first 8 bytes of its SHA-256 as hex.
pub(super) fn search_key(q: &str) -> Option<String> {
    (!q.is_empty()).then(|| {
        crate::hex::hex(
            sha2::Sha256::digest(q.as_bytes())
                .get(..8)
                .unwrap_or_default(),
        )
    })
}

/// Where a page ended: the sort, filter and search it belongs to and the last row's keys.
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Cursor {
    #[serde(rename = "s")]
    pub sort: Sort,
    #[serde(rename = "f")]
    pub filter: Filter,
    /// The time key in microseconds since the epoch (`recent`, `created`).
    #[serde(rename = "t", default, skip_serializing_if = "Option::is_none")]
    pub micros: Option<i64>,
    /// The name key (`name`).
    #[serde(rename = "n", default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "i")]
    pub id: Uuid,
    /// The search's [`search_key`]; absent without one.
    #[serde(rename = "q", default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

impl Cursor {
    /// The opaque form: the JSON's bytes as lowercase hex.
    pub(super) fn encode(&self) -> String {
        crate::hex::hex(&serde_json::to_vec(self).unwrap_or_default())
    }

    /// A cursor this gateway issued for this sort, filter and search; anything else is `400 cursor_invalid`.
    pub(super) fn decode(
        text: &str,
        sort: Sort,
        filter: Filter,
        q: &str,
    ) -> Result<Self, GatewayError> {
        let invalid = || GatewayError::Invalid {
            field: "cursor",
            reason: InvalidReason::CursorInvalid,
        };
        if text.is_empty() || text.len() > 1024 || !text.len().is_multiple_of(2) {
            return Err(invalid());
        }
        let bytes = (0..text.len() / 2)
            .map(|index| u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok())
            .collect::<Option<Vec<u8>>>()
            .ok_or_else(invalid)?;
        let cursor: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let keyed = if sort == Sort::Name {
            cursor.name.is_some() && cursor.micros.is_none()
        } else {
            cursor.micros.is_some() && cursor.name.is_none()
        };
        if cursor.sort != sort
            || cursor.filter != filter
            || cursor.search != search_key(q)
            || !keyed
        {
            return Err(invalid());
        }
        Ok(cursor)
    }
}

/// A page size from the query: 1 through [`MAX_PAGE`].
pub(super) fn page_size(limit: u32) -> Result<i64, GatewayError> {
    if limit == 0 || limit > MAX_PAGE {
        return Err(GatewayError::Invalid {
            field: "limit",
            reason: InvalidReason::LimitOutOfRange,
        });
    }
    Ok(i64::from(limit))
}

/// Where a request's page starts: its size (the list's cap without `limit`), the cursor it continues from, and that
/// cursor's time key as a timestamp.
///
/// # Errors
/// `400` for a page size out of range, or a cursor that isn't this gateway's for this sort, filter and search or comes
/// without `limit`.
pub(super) fn position(
    limit: Option<u32>,
    cursor: Option<&str>,
    sort: Sort,
    filter: Filter,
    q: &str,
) -> Result<(i64, Option<Cursor>, Option<time::OffsetDateTime>), GatewayError> {
    let invalid = || GatewayError::Invalid {
        field: "cursor",
        reason: InvalidReason::CursorInvalid,
    };
    let Some(limit) = limit else {
        return if cursor.is_some() {
            Err(invalid())
        } else {
            Ok((super::endpoints::MAX_LISTED, None, None))
        };
    };
    let after = cursor
        .map(|text| Cursor::decode(text, sort, filter, q))
        .transpose()?;
    let after_time = after
        .as_ref()
        .and_then(|cursor| cursor.micros)
        .map(|micros| time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(micros) * 1000))
        .transpose()
        .map_err(|_| invalid())?;
    Ok((page_size(limit)?, after, after_time))
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
