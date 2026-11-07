//! GW-I-053: date bounds that depend on today.
use super::range;
use crate::{GatewayError, InvalidReason};
use time::macros::date;

#[test]
fn gw_i_053_tomorrow_is_the_latest_end_and_the_default_is_thirty_days() -> Result<(), GatewayError>
{
    let today = date!(2026 - 03 - 01);
    let tomorrow = range(Some("to=2026-03-02"), today)?;
    assert_eq!(
        (tomorrow.from, tomorrow.to),
        (date!(2026 - 02 - 01), date!(2026 - 03 - 02))
    );
    assert!(matches!(
        range(Some("to=2026-03-03"), today),
        Err(GatewayError::Invalid {
            field: "to",
            reason: InvalidReason::DateFuture
        })
    ));
    let default = range(None, today)?;
    assert_eq!((default.from, default.to), (date!(2026 - 01 - 31), today));
    Ok(())
}
