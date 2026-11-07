//! The task list's opaque keyset cursor (089): the last row's `(last_activity_at, id)`, so the next page starts strictly
//! after it in `last_activity_at DESC, id DESC` order and ties never skip or repeat a row.
use crate::{GatewayError, InvalidReason};
use time::OffsetDateTime;
use uuid::Uuid;

/// The cursor format; a different first byte is refused.
const VERSION: u8 = 1;
/// Version, microseconds since the epoch, and the id: 1 + 8 + 16 bytes, as 50 lowercase hex characters.
const BYTES: usize = 25;

/// Encodes the position after a row.
pub(super) fn encode(activity: OffsetDateTime, id: Uuid) -> String {
    let micros = i64::try_from(activity.unix_timestamp_nanos() / 1000).unwrap_or(i64::MAX);
    let mut bytes = Vec::with_capacity(BYTES);
    bytes.push(VERSION);
    bytes.extend_from_slice(&micros.to_be_bytes());
    bytes.extend_from_slice(id.as_bytes());
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat()
}

/// Decodes a cursor this gateway issued; anything else is `400` with the `cursor_invalid` reason.
pub(super) fn decode(cursor: &str) -> Result<(OffsetDateTime, Uuid), GatewayError> {
    let invalid = || GatewayError::Invalid {
        field: "cursor",
        reason: InvalidReason::CursorInvalid,
    };
    if cursor.len() != BYTES * 2 {
        return Err(invalid());
    }
    let bytes = (0..BYTES)
        .map(|index| {
            cursor
                .get(index * 2..index * 2 + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(invalid)?;
    let (version, rest) = bytes.split_first().ok_or_else(invalid)?;
    if *version != VERSION {
        return Err(invalid());
    }
    let (micros, id) = rest.split_at(8);
    let micros = i64::from_be_bytes(micros.try_into().map_err(|_| invalid())?);
    let activity = OffsetDateTime::from_unix_timestamp_nanos(i128::from(micros) * 1000)
        .map_err(|_| invalid())?;
    let id = Uuid::from_slice(id).map_err(|_| invalid())?;
    Ok((activity, id))
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    use time::macros::datetime;

    #[test]
    fn gw_i_058_cursors_round_trip_to_the_microsecond_and_refuse_anything_else() {
        let activity = datetime!(2026-01-01 00:00:00.123456 UTC);
        let id = uuid::Uuid::now_v7();
        let cursor = encode(activity, id);
        assert_eq!(cursor.len(), 50);
        assert!(matches!(decode(&cursor), Ok(decoded) if decoded == (activity, id)));
        let other_version = format!("02{}", &cursor[2..]);
        for bad in [
            "",
            "!!",
            "0g",
            &cursor[..48],
            &format!("{cursor}00"),
            &other_version,
        ] {
            assert!(decode(bad).is_err(), "{bad}");
        }
    }
}
